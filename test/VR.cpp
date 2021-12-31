#include <gtest/gtest.h>

#include "app/Mock.hpp"
#include "common/ListLog.hpp"
#include "replication/vr/Client.hpp"
#include "replication/vr/Replica.hpp"
#include "transport/Simulated.hpp"

using namespace oskr;     // NOLINT
using namespace oskr::vr; // NOLINT
using namespace std;      // NOLINT

class VR : public testing::Test
{
protected:
    Config<Simulated> config;
    Simulated transport;
    vector<unique_ptr<MockApp>> app;
    vector<unique_ptr<ListLog>> log;
    vector<unique_ptr<Replica<Simulated>>> replica;
    vector<unique_ptr<vr::Client<Simulated>>> client;

    VR() :
        config{1, {"replica-0", "replica-1", "replica-2", "replica-3"}, {}},
        transport(config)
    {
        for (int i = 0; i < config.n_replica(); i += 1) {
            app.push_back(make_unique<MockApp>());
            log.push_back(make_unique<ListLog>(*app.back()));
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
    debug("one request success");
}