#pragma once
#include "core/Transport.hpp"

#include <map>
#include <queue>
#include <string>
#include <unordered_map>

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
    std::map<std::uint64_t, MessageBox> message_queue;
    std::map<std::uint64_t, SequentialCallback> timeout_queue;

    // should clear up per message/timeout
    std::queue<SequentialCallback> sequential_queue;
    std::queue<ConcurrentCallback> concurrent_queue;

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

    void sendMessage(
        const TransportReceiver<SimulatedTransport> &sender,
        const Address &dest, const Message &message) override
    {
        // TODO filter
        message_queue.insert(
            {{now_us, MessageBox{sender.address, dest, message}}});
    }

    void sendMessageToMulticast(
        const TransportReceiver<SimulatedTransport> &sender,
        const Message &message) override
    {
        // TODO
    }

    void scheduleTimeout(
        std::chrono::microseconds delay, SequentialCallback callback) override
    {
        timeout_queue.insert({{now_us + delay.count(), callback}});
    }

    void scheduleConcurrent(ConcurrentCallback callback) override
    {
        concurrent_queue.push(callback);
    }
};

} // namespace oscar