#pragma once
#include <map>
#include <queue>
#include <string>
#include <unordered_map>

#include "core/Foundation.hpp"

namespace oskr
{
class Simulated;
template <> struct TransportMeta<Simulated> {
    using Address = std::string;
    using Desc = Data;
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
    std::uint64_t now_us;
    std::multimap<std::uint64_t, Callback> destiny_queue;
    int channel_id;
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
        receiver_table.insert({address, receiver});
    }

    void registerMulticastReceiver(Receiver receiver)
    {
        multicast_receiver_list.push_back(receiver);
    }

    template <typename Sender>
    void sendMessage(const Sender &sender, const Address &dest, Write write);

    void spawn(microseconds delay, Callback callback)
    {
        destiny_queue.insert(
            {now_us + delay.count(),
             [&, callback = std::move(callback)]() mutable {
                 channel_id = -1;
                 callback();
             }});
    }

    void spawn(Callback callback) { spawn(0us, std::move(callback)); }

    void spawnConcurrent(Callback callback)
    {
        destiny_queue.insert(
            {now_us, [&, callback = std::move(callback)]() mutable {
                 channel_id = 0;
                 callback();
             }});
    }

    int channel() const { return channel_id; }

    void terminate() { destiny_queue.clear(); }

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
    message.resize(write(TxSpan<BUFFER_SIZE>(message.data(), BUFFER_SIZE)));
    destiny_queue.insert({now_us + delay.count(), [&, dest, message]() mutable {
                              channel_id = -2;
                              // TODO multicast
                              receiver_table.at(dest)(sender.address, message);
                          }});
}

void Simulated::run(microseconds time_limit)
{
    while (true) {
        if (microseconds(now_us) >= time_limit) {
            panic(
                "Hard time limit reached: {}ms",
                double(time_limit.count()) / 1000);
        }

        auto timeout_iter = destiny_queue.begin();
        if (timeout_iter == destiny_queue.end()) {
            return;
        }
        now_us = timeout_iter->first;
        timeout_iter->second();
        destiny_queue.erase(timeout_iter);
    }
}

} // namespace oskr