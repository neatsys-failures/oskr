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
    template <typename T> using ref_wrapper = std::reference_wrapper<T>;
    std::vector<ref_wrapper<TransportMulticastReceiver<SimulatedTransport>>>
        multicast_receiver_list;

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
    using microseconds = std::chrono::microseconds;
    // we do not provide message content here, because serialized message is
    // normally hard to inspect
    using Filter = std::function<bool(
        const Address &source, const Address &dest, microseconds &delay)>;

private:
    std::map<int, Filter> filter_table;

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
        TransportMulticastReceiver<SimulatedTransport> &receiver) override
    {
        multicast_receiver_list.push_back(receiver);
    }

    void sendMessage(
        const TransportReceiver<SimulatedTransport> &sender,
        const Address &dest, Write write) override;

    void scheduleTimeout(microseconds delay, Callback callback) override
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

    void run(microseconds time_limit = 10 * 1000ms);

    void addFilter(int filter_id, Filter filter)
    {
        filter_table.insert({filter_id, filter});
    }

    void removeFilter(int removed_id) { filter_table.erase(removed_id); }
};

void SimulatedTransport::sendMessage(
    const TransportReceiver<SimulatedTransport> &sender, const Address &dest,
    Write write)
{
    if (!receiver_table.count(dest)) {
        panic("Send to unknown destination: sender = {}", sender.address);
    }

    microseconds delay = 0us;
    for (auto pair : filter_table) {
        if (!pair.second(sender.address, dest, delay)) {
            info("Message dropped: filter id = {}", pair.first);
            return;
        }
    }
    if (delay != 0us) {
        info("Message delayed: {}us", delay.count());
    }

    Data message(BUFFER_SIZE);
    message.resize(write(*(Buffer<BUFFER_SIZE> *)message.data()));
    message_queue.insert(
        {{now_us + delay.count(), MessageBox{sender.address, dest, message}}});
}

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

void SimulatedTransport::run(microseconds time_limit)
{
    while (true) {
        if (microseconds(now_us) >= time_limit) {
            panic(
                "Hard time limit reached: {}ms",
                double(time_limit.count()) / 1000);
        }

        auto message_iter = message_queue.begin();
        while (message_iter != message_queue.end() &&
               message_iter->first == now_us) {
            auto box = message_iter->second;
            message_queue.erase(message_iter);

            if (box.dest == config.multicast_address) {
                panic("TODO");
            }

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