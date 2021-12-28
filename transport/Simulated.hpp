#pragma once
#include <map>
#include <queue>
#include <string>
#include <unordered_map>

#include "core/Foundation.hpp"

namespace oscar
{
class Simulated;
template <> struct TransportMeta<Simulated> {
    using Address = std::string;
    static constexpr std::size_t BUFFER_SIZE = 9000; // TODO configurable
};

class Simulated : public TransportBase<Simulated>
{
public:
    using microseconds = std::chrono::microseconds;
    // we do not provide message content here, because serialized message is
    // normally hard to inspect
    using Filter = std::function<bool(
        const Address &source, const Address &dest, microseconds &delay)>;

private:
    std::unordered_map<Address, Receiver> receiver_table;
    std::vector<Receiver> multicast_receiver_list;

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
    std::map<int, Filter> filter_table;

public:
    const Config<Simulated> &config;

    Simulated(const Config<Simulated> &config) : config(config) { now_us = 0; }

    Address allocateAddress()
    {
        Address addr("client-");
        addr.push_back('A' + (char)receiver_table.size());
        return addr;
    }

    void registerReceiver(Address address, Receiver receiver)
    {
        receiver_table.insert({{address, receiver}});
    }

    void registerMulticastReceiver(Receiver receiver)
    {
        multicast_receiver_list.push_back(receiver);
    }

    template <typename Sender>
    void sendMessage(const Sender &sender, const Address &dest, Write write);

    void spawn(microseconds delay, Callback callback)
    {
        timeout_queue.insert({{now_us + delay.count(), callback}});
    }

    void spawn(Callback callback) { sequential_queue.push(callback); }

    void spawnConcurrent(Callback callback) { concurrent_queue.push(callback); }

    int channel() const { return concurrent_id; }

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

template <typename Sender>
void Simulated::sendMessage(
    const Sender &sender, const Address &dest, Write write)
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

void Simulated::processScheduled()
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

void Simulated::run(microseconds time_limit)
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
            receiver_table.at(box.dest)(
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