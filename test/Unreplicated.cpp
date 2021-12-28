#include <random>
#include <vector>

#include <gtest/gtest.h>

#include "app/Mock.hpp"
#include "common/BasicClient.hpp"
#include "common/ListLog.hpp"
#include "replication/unreplicated/Replica.hpp"
#include "transport/Simulated.hpp"

using namespace oscar;               // NOLINT
using namespace oscar::unreplicated; // NOLINT

class Unreplicated : public testing::Test
{
protected:
    Config<Simulated> config;
    Simulated transport;
    MockApp app;
    ListLog log;
    Replica<Simulated> replica;

    Unreplicated() :
        config{0, {"replica-0"}}, transport(config), log(app),
        replica(transport, log)
    {
    }

    std::vector<BasicClient<Simulated, ReplicaMessage>> client;

    void spawnClient(int n_client)
    {
        client.reserve(n_client);
        for (int i = 0; i < n_client; i += 1) {
            client.push_back(BasicClient<Simulated, ReplicaMessage>(
                transport,
                {BasicClient<>::Config::Strategy::PRIMARY_FIRST, 1000ms, 1}));
        }
    }
};

TEST_F(Unreplicated, Noop) {}

TEST_F(Unreplicated, OneRequest)
{
    spawnClient(1);

    std::string op_string("Test operation");
    Data op(op_string.begin(), op_string.end());
    bool completed = false;

    transport.spawn(0ms, [&] {
        client[0].invoke(op, [&](Data result) {
            ASSERT_EQ(
                std::string(result.begin(), result.end()),
                "Re: Test operation");
            completed = true;
        });
    });

    transport.run();
    ASSERT_TRUE(completed);
    ASSERT_EQ(app.op_list.size(), 1);
    ASSERT_EQ(app.op_list[0], op);
}

TEST_F(Unreplicated, TenClientOneRequest)
{
    spawnClient(10);

    std::string op_string("Test operation");
    Data op(op_string.begin(), op_string.end());
    int n_completed = 0;

    for (int i = 0; i < 10; i += 1) {
        transport.spawn(0ms, [&, i] {
            client[i].invoke(op, [&](Data result) {
                ASSERT_EQ(
                    std::string(result.begin(), result.end()),
                    "Re: Test operation");
                n_completed += 1;
            });
        });
    }

    transport.run();
    ASSERT_EQ(n_completed, 10);
    ASSERT_EQ(app.op_list.size(), 10);
}

TEST_F(Unreplicated, TenClientOneSecond)
{
    spawnClient(10);
    std::uniform_int_distribution dist(0, 50);

    bool time_up = false;
    int n_completed = 0;
    Simulated::Callback close_loop[10];
    for (int i = 0; i < 10; i += 1) {
        close_loop[i] = [&, i] {
            if (time_up) {
                return;
            }
            transport.spawn(
                std::chrono::milliseconds(dist(random_engine())), [&, i] {
                    debug(
                        "i = {}, client[i] = {}", i,
                        reinterpret_cast<void *>(&client[i]));
                    client[i].invoke(Data(), [&, i](auto) {
                        n_completed += 1;
                        close_loop[i]();
                    });
                });
        };
        transport.spawn(0ms, close_loop[i]);
    }
    transport.spawn(1000ms, [&] { time_up = true; });

    transport.run();
    ASSERT_EQ(app.op_list.size(), n_completed);
    ASSERT_GE(app.op_list.size(), 10 * 20);
}

TEST_F(Unreplicated, ResendUndone)
{
    spawnClient(1);
    bool completed = false;
    transport.spawn(0us, [&] {
        transport.addFilter(1, [](auto, auto, auto) { return false; });
    });
    transport.spawn(10us, [&] {
        client[0].invoke(Data(), [&](auto) { completed = true; });
    });
    transport.spawn(20us, [&] { transport.removeFilter(1); });
    transport.run();
    ASSERT_TRUE(completed);
}

TEST_F(Unreplicated, ResendDuplicated)
{
    spawnClient(1);
    bool completed = false;
    transport.spawn(0us, [&] {
        transport.addFilter(1, [&](const auto &source, auto, auto) {
            return source != config.replica_address_list[0];
        });
    });
    transport.spawn(10us, [&] {
        client[0].invoke(Data(), [&](auto) { completed = true; });
    });
    transport.spawn(20us, [&] { transport.removeFilter(1); });
    transport.spawn(30us, [&] { ASSERT_FALSE(completed); });
    transport.run();
    ASSERT_TRUE(completed);
    ASSERT_EQ(app.op_list.size(), 1);
}
