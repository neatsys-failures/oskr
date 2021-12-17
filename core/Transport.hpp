#pragma once
#include "core/Config.hpp"
#include "core/TransportReceiver.hpp"

#include <chrono>
#include <functional>
#include <vector>

namespace oscar
{

template <typename Transport> struct AddressTrait {
};

template <typename Self> class Transport
{
protected:
    Transport(const Config<Self> &config) : config(config) {}

public:
    using Address = typename AddressTrait<Self>::Type;

    const Config<Self> &config;

    virtual void registerReceiver(TransportReceiver<Self> &receiver) = 0;
    virtual void
    registerMulticastReceiver(TransportReceiver<Self> &receiver) = 0;

    using SequentialCallback = std::function<void()>;
    virtual void scheduleTimeout(
        std::chrono::microseconds delay, SequentialCallback callback) = 0;

    virtual void scheduleSequential(SequentialCallback callback)
    {
        scheduleTimeout(std::chrono::microseconds(0), callback);
    }

    using ConcurrentCallback = std::function<void(int)>;
    virtual void scheduleConcurrent(ConcurrentCallback callback) = 0;

    using Message = std::vector<std::uint8_t>;
    virtual void sendMessage(
        const TransportReceiver<Self> &sender, const Address &dest,
        const Message &message) = 0;

    void sendMessageToReplica(
        const TransportReceiver<Self> &sender, int replica_id,
        const Message &message)
    {
        sendMessage(sender, config.replica_address_list[replica_id], message);
    }

    void sendMessageToAll(
        const TransportReceiver<Self> &sender, const Message &message)
    {
        for (auto address : config.replica_address_list) {
            if (address != sender.address) {
                sendMessage(sender, address, message);
            }
        }
    }

    virtual void sendMessageToMulticast(
        const TransportReceiver<Self> &sender, const Message &message) = 0;
};

} // namespace oscar