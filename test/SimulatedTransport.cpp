#include <cstring>

#include <gtest/gtest.h>
#include <spdlog/spdlog.h>

#include "transport/Simulated.hpp"

using namespace oscar;
using namespace std::chrono_literals;

TEST(SimulatedTransport, ExternalTimeout)
{
    Config<SimulatedTransport> config{0, {}, {}};
    SimulatedTransport transport(config);
    bool triggered = false;
    transport.scheduleTimeout(0us, [&]() { triggered = true; });
    transport.run();
    ASSERT_TRUE(triggered);
}

template <typename Transport>
class SimpleReceiver : public TransportReceiver<Transport>
{
public:
    SimpleReceiver(typename Transport::Address address) :
        TransportReceiver<Transport>(address)
    {
    }

    typename Transport::Address latest_remote;
    Data latest_message;

    void receiveMessage(
        const typename Transport::Address &remote, const Span &buffer) override
    {
        latest_remote = remote;
        latest_message = Data(buffer.begin(), buffer.end());
    }
};

TEST(SimulatedTransport, OneMessage)
{
    Config<SimulatedTransport> config{0, {}, {}};
    SimulatedTransport transport(config);
    SimpleReceiver<SimulatedTransport> receiver1("receiver-1"),
        receiver2("receiver-2");
    transport.registerReceiver(receiver1);
    Data message{0, 1, 2, 3};
    transport.scheduleTimeout(0us, [&]() {
        transport.sendMessage(
            receiver2, "receiver-1", [&](std::uint8_t *buffer) {
                std::memcpy(buffer, message.data(), message.size());
                return message.size();
            });
    });
    transport.run();
    ASSERT_EQ(receiver1.latest_remote, "receiver-2");
    ASSERT_EQ(receiver1.latest_message, message);
}

template <typename Transport>
class PingPongReceiver : public TransportReceiver<Transport>
{
    Transport &transport;
    std::function<void(PingPongReceiver<Transport> &)> on_exit;
    int delay_us;

public:
    PingPongReceiver(
        typename Transport::Address address, Transport &transport,
        std::function<void(PingPongReceiver<Transport> &)> on_exit,
        int delay_us) :
        TransportReceiver<Transport>(address),
        transport(transport)
    {
        this->on_exit = on_exit;
        this->delay_us = delay_us;
    }

    void receiveMessage(
        const typename Transport::Address &remote, const Span &buffer) override
    {
        if (buffer.size() == 100) {
            on_exit(*this);
            return;
        }

        Data reply(buffer.begin(), buffer.end());
        reply.push_back(buffer.size());
        auto write = [reply](auto &buffer) {
            std::memcpy(buffer, reply.data(), reply.size());
            return reply.size();
        };

        if (delay_us == 0) {
            transport.sendMessage(*this, remote, write);
        } else {
            transport.scheduleTimeout(
                std::chrono::microseconds(delay_us), [this, remote, write] {
                    transport.sendMessage(*this, remote, write);
                });
        }
    }

    void Start()
    {
        transport.sendMessageToAll(*this, [](auto &buffer) {
            (void)buffer;
            return 0;
        });
    }
};

TEST(SimulatedTransport, PingPong)
{
    Config<SimulatedTransport> config{0, {"ping", "pong"}, {}};
    SimulatedTransport transport(config);
    bool all_done = false;
    auto on_exit = [&](const PingPongReceiver<SimulatedTransport> &receiver) {
        all_done = true;
        ASSERT_EQ(receiver.address, "pong");
    };
    PingPongReceiver<SimulatedTransport> ping("ping", transport, on_exit, 0);
    PingPongReceiver<SimulatedTransport> pong("pong", transport, on_exit, 0);
    transport.registerReceiver(ping);
    transport.registerReceiver(pong);
    transport.scheduleTimeout(0us, [&] { ping.Start(); });
    spdlog::debug("Transport run");
    transport.run();
    ASSERT_TRUE(all_done);
}

TEST(SimulatedTransport, PingPongWithTimeout)
{
    Config<SimulatedTransport> config{0, {"ping", "pong"}, {}};
    SimulatedTransport transport(config);
    bool all_done = false;
    auto on_exit = [&](const PingPongReceiver<SimulatedTransport> &receiver) {
        all_done = true;
        ASSERT_EQ(receiver.address, "pong");
    };
    PingPongReceiver<SimulatedTransport> ping("ping", transport, on_exit, 1);
    PingPongReceiver<SimulatedTransport> pong("pong", transport, on_exit, 2);
    transport.registerReceiver(ping);
    transport.registerReceiver(pong);
    transport.scheduleTimeout(0us, [&] { ping.Start(); });
    transport.run();
    ASSERT_TRUE(all_done);
}
