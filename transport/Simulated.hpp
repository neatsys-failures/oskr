#pragma once
#include <map>
#include <queue>
#include <string>
#include <unordered_map>

#include <spdlog/spdlog.h>

#include "core/Transport.hpp"
#include "core/TransportReceiver.hpp"

namespace oscar
{

class SimulatedTransport;
template <> struct AddressTrait<SimulatedTransport> {
    using Type = std::string;
};

class SimulatedTransport : public Transport<SimulatedTransport>
{
    std::unordered_map<Address, TransportReceiver<SimulatedTransport> &>
        receiver_table;

    struct MessageBox {
        Address source, dest;
        Message message;
    };
    std::uint64_t now_us;
    std::multimap<std::uint64_t, MessageBox> message_queue;
    std::multimap<std::uint64_t, Callback> timeout_queue;

    // should clear up per message/timeout
    std::queue<Callback> sequential_queue, concurrent_queue;
    int concurrent_id;

public:
    SimulatedTransport(const Config<SimulatedTransport> &config)
        : Transport(config)
    {
        now_us = 0;
    }

    void
    registerReceiver(TransportReceiver<SimulatedTransport> &receiver) override
    {
        receiver_table.insert({{receiver.address, receiver}});
    }

    void registerMulticastReceiver(
        TransportReceiver<SimulatedTransport> &receiver) override
    {
        // TODO
    }

    void sendMessage(
        const TransportReceiver<SimulatedTransport> &sender,
        const Address &dest, const Message &message) override
    {
        // TODO filter
        message_queue.insert(
            {{now_us, MessageBox{sender.address, dest, message}}});
    }

    void
    scheduleTimeout(std::chrono::microseconds delay, Callback callback) override
    {
        timeout_queue.insert({{now_us + delay.count(), callback}});
    }

    void scheduleSequential(Callback callback) override
    {
        sequential_queue.push(callback);
    }

    void scheduleConcurrent(Callback callback) override
    {
        concurrent_queue.push(callback);
    }

    int getConcurrentId() const override { return concurrent_id; }

private:
    void processScheduled()
    {
        while (true) {
            if (!concurrent_queue.empty()) {
                concurrent_id = 0;
                concurrent_queue.front()();
                concurrent_queue.pop();
            } else if (!sequential_queue.empty()) {
                concurrent_id = -1;
                sequential_queue.front()();
                sequential_queue.pop();
            } else {
                return;
            }
        }
    }

public:
    void clearTimeout() { timeout_queue.clear(); }

    void run()
    {
        while (true) {
            auto message_iter = message_queue.begin();
            while (message_iter != message_queue.end() &&
                   message_iter->first == now_us) {
                auto box = message_iter->second;
                message_queue.erase(message_iter);

                // TODO multicast message

                concurrent_id = 0;
                receiver_table.at(box.dest).receiveMessage(
                    box.source, box.message);
                processScheduled();
                message_iter = message_queue.begin();
            }

            auto timeout_iter = timeout_queue.begin();
            if (timeout_iter != timeout_queue.end() &&
                (message_iter == message_queue.end() ||
                 timeout_iter->first < message_iter->first)) {
                now_us = timeout_iter->first;
                concurrent_id = 0;
                timeout_iter->second();
                processScheduled();
                timeout_queue.erase(timeout_iter);
            } else if (message_iter != message_queue.end()) {
                now_us = message_iter->first;
            } else {
                return;
            }
        }
    }
};

} // namespace oscar