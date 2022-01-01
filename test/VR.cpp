#include <gtest/gtest.h>

#include "app/Mock.hpp"
#include "common/ListLog.hpp"
#include "replication/vr/Client.hpp"
#include "replication/vr/Replica.hpp"
#include "transport/Simulated.hpp"

using namespace oskr;     // NOLINT
using namespace oskr::vr; // NOLINT
using namespace std;      // NOLINT

class VRLog : public ListLog
{
public:
    explicit VRLog(App &app) : ListLog(app) {}

    static void assertConsistent(
        const vector<unique_ptr<VRLog>> &log, Config<Simulated> &config)
    {
        for (OpNumber i = 1;; i += 1) {
            bool completed = false;
            assertConsistentOp(log, i, config, completed);
            if (completed) {
                return;
            }
        }
    }

private:
    static void assertConsistentOp(
        const vector<unique_ptr<VRLog>> &log, OpNumber index,
        Config<Simulated> &config, bool &completed)
    {
        int sample_id = -1;
        for (size_t i = 0; i < log.size(); i += 1) {
            if (log[i]->blockOffset(index) < log[i]->block_list.size()) {
                sample_id = static_cast<int>(i);
                break;
            }
        }
        if (sample_id == -1) {
            completed = true;
            return;
        }
        FlattenBlock &sample_block =
            log[sample_id]->block_list[log[sample_id]->blockOffset(index)];

        int n_prepared = 0, n_committed = 0;
        for (size_t i = 0; i < log.size(); i += 1) {
            if (log[i]->blockOffset(index) >= log[i]->block_list.size()) {
                continue;
            }
            FlattenBlock &block =
                log[i]->block_list[log[i]->blockOffset(index)];
            ASSERT_EQ(block.offset, sample_block.offset) << fmt::format(
                "block not match: op number = {}, sampled id = {}, compared id "
                "= {}",
                index, sample_id, i);
            ASSERT_EQ(block.n_entry, sample_block.n_entry) << fmt::format(
                "block not match: op number = {}, sampled id = {}, compared id "
                "= {}",
                index, sample_id, i);
            // TODO(sgdxbc) do not assume entry content matches (not useful
            // without BFT)

            n_prepared += 1;
            if (block.committed) {
                n_committed += 1;
            }
        }

        if (n_committed > 0) {
            ASSERT_GE(n_prepared, config.n_fault + 1) << fmt::format(
                "block committed without quorum prepared: op number = {}",
                index);
        }
    }
};

class VR : public testing::Test
{
protected:
    Config<Simulated> config;
    Simulated transport;
    vector<unique_ptr<MockApp>> app;
    vector<unique_ptr<VRLog>> log;
    vector<unique_ptr<Replica<Simulated>>> replica;
    vector<unique_ptr<vr::Client<Simulated>>> client;

    VR() :
        config{1, {"replica-0", "replica-1", "replica-2"}, {}},
        transport(config)
    {
        for (int i = 0; i < config.n_replica(); i += 1) {
            app.push_back(make_unique<MockApp>());
            log.push_back(make_unique<VRLog>(*app.back()));
            replica.push_back(
                make_unique<Replica<Simulated>>(transport, *log.back(), i, 1));
        }
    }

    void spawnClient(int n_client)
    {
        for (int i = 0; i < n_client; i += 1) {
            client.push_back(make_unique<vr::Client<Simulated>>(transport));
        }
    }
};

TEST_F(VR, Noop) { spawnClient(1); }
TEST_F(VR, OneRequest)
{
    spawnClient(1);
    string op_string{"One request"};
    bool checked = false;
    transport.spawn(0ms, [&] {
        client[0]->invoke(
            Data(op_string.begin(), op_string.end()), [&](auto result) {
                ASSERT_EQ(
                    string(result.begin(), result.end()), "Re: One request");
                checked = true;
                transport.terminate();
            });
    });
    transport.run();
    ASSERT_TRUE(checked);
    debug("one request finished");
    VRLog::assertConsistent(log, config);
}

TEST_F(VR, TenRequest)
{
    spawnClient(1);
    int i = 0;
    Fn<void()> close_loop = [&] {
        client[0]->invoke(Data(), [&](auto) {
            i += 1;
            if (i == 10) {
                transport.terminate();
                return;
            }
            close_loop();
        });
    };
    transport.spawn(0ms, [&] { close_loop(); });
    transport.run();
    ASSERT_EQ(app[0]->op_list.size(), 10);
    VRLog::assertConsistent(log, config);
}

TEST_F(VR, EventuallyAllCommit)
{
    spawnClient(1);
    transport.spawn(0ms, [&] { client[0]->invoke(Data(), [&](auto) {}); });
    transport.spawn(210ms, [&] { transport.terminate(); });
    transport.run();
    for (size_t i = 0; i < app.size(); i += 1) {
        ASSERT_EQ(app[i]->op_list.size(), 1);
    }
}

TEST_F(VR, ViewChange)
{
    spawnClient(1);
    transport.spawn(0ms, [&] {
        transport.addFilter(1, [&](auto &source, auto &dest, auto &) {
            return source != config.replica_address_list[0] &&
                   dest != config.replica_address_list[0];
        });
    });
    bool completed = false;
    transport.spawn(10ms, [&] {
        client[0]->invoke(Data(), [&](auto) {
            completed = true;
            transport.terminate();
        });
    });
    transport.run();
    ASSERT_TRUE(completed);
}

TEST_F(VR, NoResendAfterViewChange)
{
    spawnClient(1);
    transport.spawn(0ms, [&] {
        transport.addFilter(1, [&](auto &source, auto &dest, auto &) {
            return source != config.replica_address_list[0] &&
                   dest != config.replica_address_list[0];
        });
    });
    bool completed = false;
    transport.spawn(10ms, [&] {
        client[0]->invoke(Data(), [&](auto) {
            client[0]->invoke(Data(), [&](auto) { completed = true; });
        });
    });
    transport.spawn(1020ms, [&] { transport.terminate(); });
    transport.run();
    ASSERT_TRUE(completed);
}