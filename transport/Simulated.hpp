#pragma once
#include <map>
#include <queue>
#include <string>
#include <unordered_map>

#include "core/Foundation.hpp"

namespace oscar
{

class SimulatedTransport;
template <> struct TransportMeta<SimulatedTransport> {
    using Address = std::string;
    static constexpr std::size_t BUFFER_SIZE = 9000; // TODO configurable
};

class SimulatedTransport : public Transport<SimulatedTransport>
{
    std::unordered_map<Address, TransportReceiver<SimulatedTransport> &>
        receiver_table;

    struct MessageBox {
        Address source, dest;
        Data message;
    };
    std::uint64_t now_us;
    std::multimap<std::uint64_t, MessageBox> message_queue;
    std::multimap<std::uint64_t, Callback> timeout_queue;

    // should clear up per message/timeout
    std::queue<Callback> sequential_queue, concurrent_queue;
    int concurrent_id;

public:
    SimulatedTransport(const Config<SimulatedTransport> &config) :
        Transport(config)
    {
        now_us = 0;
    }

    Address allocateAddress() override
    {
        Address addr("client-");
        addr.push_back('A' + (char)receiver_table.size());
        return addr;
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
        (void)receiver;
    }

    void sendMessage(
        const TransportReceiver<SimulatedTransport> &sender,
        const Address &dest, Write write) override
    {
        if (!receiver_table.count(dest)) {
            panic("Send to unknown destination: sender = {}", sender.address);
        }

        // TODO filter
        Data message(BUFFER_SIZE);
        message.resize(write(*(Buffer<BUFFER_SIZE> *)message.data()));
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
    void processScheduled();

public:
    void clearTimeout() { timeout_queue.clear(); }

    void run();
};

void SimulatedTransport::processScheduled()
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

void SimulatedTransport::run()
{
    while (true) {
        auto message_iter = message_queue.begin();
        while (message_iter != message_queue.end() &&
               message_iter->first == now_us) {
            auto box = message_iter->second;
            message_queue.erase(message_iter);

            // TODO multicast message

            concurrent_id = -1;
            receiver_table.at(box.dest).receiveMessage(
                box.source, Span(box.message.data(), box.message.size()));
            processScheduled();
            message_iter = message_queue.begin();
        }

        auto timeout_iter = timeout_queue.begin();
        if (timeout_iter != timeout_queue.end() &&
            (message_iter == message_queue.end() ||
             timeout_iter->first < message_iter->first)) {
            now_us = timeout_iter->first;
            concurrent_id = -1;
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

} // namespace oscar