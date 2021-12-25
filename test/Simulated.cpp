#include <cstring>

#include <gtest/gtest.h>
#include <spdlog/spdlog.h>

#include "transport/Simulated.hpp"

using namespace oscar; // NOLINT

TEST(Simulated, ExternalTimeout)
{
    Config<Simulated> config{0, {}, {}};
    Simulated transport(config);
    bool triggered = false;
    transport.scheduleTimeout(0us, [&]() { triggered = true; });
    transport.run();
    ASSERT_TRUE(triggered);
}

template <typename Transport>
class SimpleReceiver : public TransportReceiver<Transport>
{
public:
    typename Transport::Address latest_remote;
    Data latest_message;
    int n_message;

    explicit SimpleReceiver(typename Transport::Address address) :
        TransportReceiver<Transport>(address)
    {
        n_message = 0;
    }

    void receiveMessage(
        const typename Transport::Address &remote, Span buffer) override
    {
        n_message += 1;
        latest_remote = remote;
        latest_message = Data(buffer.begin(), buffer.end());
    }
};

TEST(Simulated, OneMessage)
{
    Config<Simulated> config{0, {}, {}};
    Simulated transport(config);
    SimpleReceiver<Simulated> receiver1("receiver-1"), receiver2("receiver-2");
    transport.registerReceiver(receiver1);
    Data message{0, 1, 2, 3};
    transport.scheduleTimeout(0us, [&]() {
        transport.sendMessage(receiver2, "receiver-1", [&](auto &buffer) {
            std::copy(message.begin(), message.end(), buffer);
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
    std::chrono::microseconds delay_us;

public:
    PingPongReceiver(
        typename Transport::Address address, Transport &transport,
        std::function<void(PingPongReceiver<Transport> &)> on_exit,
        std::chrono::microseconds delay_us) :
        TransportReceiver<Transport>(address),
        transport(transport)
    {
        this->on_exit = on_exit;
        this->delay_us = delay_us;
    }

    void receiveMessage(
        const typename Transport::Address &remote, Span span) override
    {
        if (span.size() == 100) {
            on_exit(*this);
            return;
        }

        Data reply(span.begin(), span.end());
        reply.push_back(span.size());
        auto write = [reply](auto &buffer) {
            std::memcpy(buffer, reply.data(), reply.size());
            return reply.size();
        };

        if (delay_us == 0us) {
            transport.sendMessage(*this, remote, write);
        } else {
            transport.scheduleTimeout(delay_us, [this, remote, write] {
                transport.sendMessage(*this, remote, write);
            });
        }
    }

    void Start()
    {
        transport.sendMessageToAll(*this, [](auto &) { return 0; });
    }
};

TEST(Simulated, PingPong)
{
    Config<Simulated> config{0, {"ping", "pong"}, {}};
    Simulated transport(config);
    bool all_done = false;
    auto on_exit = [&](const PingPongReceiver<Simulated> &receiver) {
        all_done = true;
        ASSERT_EQ(receiver.address, "pong");
    };
    PingPongReceiver<Simulated> ping("ping", transport, on_exit, 0us);
    PingPongReceiver<Simulated> pong("pong", transport, on_exit, 0us);
    transport.registerReceiver(ping);
    transport.registerReceiver(pong);
    transport.scheduleTimeout(0us, [&] { ping.Start(); });
    spdlog::debug("Transport run");
    transport.run();
    ASSERT_TRUE(all_done);
}

TEST(Simulated, PingPongWithTimeout)
{
    Config<Simulated> config{0, {"ping", "pong"}, {}};
    Simulated transport(config);
    bool all_done = false;
    auto on_exit = [&](const PingPongReceiver<Simulated> &receiver) {
        all_done = true;
        ASSERT_EQ(receiver.address, "pong");
    };
    PingPongReceiver<Simulated> ping("ping", transport, on_exit, 1us);
    PingPongReceiver<Simulated> pong("pong", transport, on_exit, 2us);
    transport.registerReceiver(ping);
    transport.registerReceiver(pong);
    transport.scheduleTimeout(0us, [&] { ping.Start(); });
    bool checked = false;
    transport.scheduleTimeout(200us, [&] {
        ASSERT_TRUE(all_done);
        checked = true;
    });
    transport.run();
    ASSERT_TRUE(checked);
}

TEST(Simulated, DropMessage)
{
    Config<Simulated> config{0, {}};
    Simulated transport(config);
    SimpleReceiver<Simulated> receiver1("receiver-1"), receiver2("receiver-2");
    transport.registerReceiver(receiver1);
    transport.registerReceiver(receiver2);

    for (auto i = 0us; i < 10us; i += 1us) {
        transport.scheduleTimeout(i, [&]() {
            transport.sendMessage(receiver2, "receiver-1", [&](auto &buffer) {
                std::string message("Bad network");
                std::copy(message.begin(), message.end(), buffer);
                return message.size();
            });
        });
        transport.scheduleTimeout(i, [&]() {
            transport.sendMessage(receiver1, "receiver-2", [&](auto &buffer) {
                std::string message("Good network");
                std::copy(message.begin(), message.end(), buffer);
                return message.size();
            });
        });
    }
    transport.addFilter(1, [](const auto &, const auto &dest, auto &) {
        return dest != "receiver-1";
    });
    bool checked = false;
    transport.scheduleTimeout(20us, [&] {
        ASSERT_EQ(receiver1.n_message, 0);
        ASSERT_EQ(receiver2.n_message, 10);
        checked = true;
    });

    transport.run();
    ASSERT_TRUE(checked);
}

TEST(Simulated, DelayMessage)
{
    Config<Simulated> config{0, {}};
    Simulated transport(config);
    SimpleReceiver<Simulated> receiver1("receiver-1"), receiver2("receiver-2");
    transport.registerReceiver(receiver1);
    transport.registerReceiver(receiver2);

    for (auto i = 0us; i < 10us; i += 1us) {
        transport.scheduleTimeout(i, [&]() {
            transport.sendMessage(receiver2, "receiver-1", [&](auto &buffer) {
                std::string message("Slow network");
                std::copy(message.begin(), message.end(), buffer);
                return message.size();
            });
        });
        transport.scheduleTimeout(i, [&]() {
            transport.sendMessage(receiver1, "receiver-2", [&](auto &buffer) {
                std::string message("Good network");
                std::copy(message.begin(), message.end(), buffer);
                return message.size();
            });
        });
    }
    transport.addFilter(1, [](const auto &, const auto &dest, auto &delay) {
        if (dest == "receiver-1") {
            delay += 50us;
        }
        return true;
    });
    bool checked = false;
    transport.scheduleTimeout(20us, [&] {
        ASSERT_EQ(receiver1.n_message, 0);
        ASSERT_EQ(receiver2.n_message, 10);
        transport.scheduleTimeout(80us, [&] {
            ASSERT_EQ(receiver1.n_message, 10);
            ASSERT_EQ(receiver2.n_message, 10);
            checked = true;
        });
    });

    transport.run();
    ASSERT_TRUE(checked);
}