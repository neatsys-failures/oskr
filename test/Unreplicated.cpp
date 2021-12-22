#include <random>
#include <vector>

#include <gtest/gtest.h>

#include "app/Mock.hpp"
#include "common/BasicClient.hpp"
#include "common/ListLog.hpp"
#include "replication/unreplicated/Replica.hpp"
#include "transport/Simulated.hpp"

using namespace oscar;
using namespace oscar::unreplicated;
using namespace std::literals::chrono_literals;
using Strategy =
    BasicClient<SimulatedTransport, ReplicaMessage>::Config::Strategy;

class Unreplicated : public testing::Test
{
protected:
    Config<SimulatedTransport> config;
    SimulatedTransport transport;
    MockApp app;
    ListLog log;
    Replica<SimulatedTransport> replica;

    Unreplicated() :
        config{0, {"replica-0"}, {}}, transport(config), log(app),
        replica(transport, log)
    {
        transport.registerReceiver(replica);
    }

    std::vector<BasicClient<SimulatedTransport, ReplicaMessage>> client;

    void spawnClient(int n_client)
    {
        client.reserve(n_client);
        for (int i = 0; i < n_client; i += 1) {
            client.push_back(BasicClient<SimulatedTransport, ReplicaMessage>(
                transport, {Strategy::PRIMARY_FIRST, 1000ms, 1}));
            transport.registerReceiver(client.back());
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

    transport.scheduleTimeout(0ms, [&] {
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
        transport.scheduleTimeout(0ms, [&, i] {
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
    std::random_device r;
    std::default_random_engine engine(r());
    std::uniform_int_distribution dist(0, 50);

    bool time_up = false;
    int n_completed = 0;
    SimulatedTransport::Callback close_loop[10];
    for (int i = 0; i < 10; i += 1) {
        close_loop[i] = [&, i] {
            if (time_up) {
                return;
            }
            transport.scheduleTimeout(
                std::chrono::milliseconds(dist(engine)), [&, i] {
                    debug("i = {}, client[i] = {}", i, (void *)&client[i]);
                    client[i].invoke(Data(), [&, i](auto) {
                        n_completed += 1;
                        close_loop[i]();
                    });
                });
        };
        transport.scheduleTimeout(0ms, close_loop[i]);
    }
    transport.scheduleTimeout(1000ms, [&] { time_up = true; });

    transport.run();
    ASSERT_EQ(app.op_list.size(), n_completed);
    ASSERT_GE(app.op_list.size(), 10 * 20);
}
