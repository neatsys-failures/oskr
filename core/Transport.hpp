#pragma once
#include "core/Config.hpp"
#include "core/TransportReceiver.hpp"
#include "core/Type.hpp"

#include <chrono>
#include <functional>
#include <vector>

namespace oscar
{
template <typename Transport> struct AddressTrait {
    // using Type = ...
};
template <typename Transport> struct BufferSizeTrait {
    // static constexpr size_t N = ...
};

template <typename Transport> class TransportReceiver;

template <typename Self> class Transport
{
protected:
    Transport(const Config<Self> &config) : config(config) {}

public:
    virtual ~Transport() {}

    using Address = typename AddressTrait<Self>::Type;
    virtual Address allocateAddress() = 0;

    const Config<Self> &config;

    virtual void registerReceiver(TransportReceiver<Self> &receiver) = 0;
    virtual void
    registerMulticastReceiver(TransportReceiver<Self> &receiver) = 0;

    using Callback = std::function<void()>;
    virtual void
    scheduleTimeout(std::chrono::microseconds delay, Callback callback) = 0;

    virtual void scheduleSequential(Callback callback)
    {
        scheduleTimeout(std::chrono::microseconds(0), callback);
    }

    virtual void scheduleConcurrent(Callback callback) = 0;

    virtual int getConcurrentId() const = 0;

    static constexpr std::size_t BUFFER_SIZE = BufferSizeTrait<Self>::N;
    using Buffer = oscar::Buffer<BUFFER_SIZE>;
    using Write = std::function<std::size_t(Buffer &buffer)>;
    virtual void sendMessage(
        const TransportReceiver<Self> &sender, const Address &dest,
        Write write) = 0;

    void sendMessageToReplica(
        const TransportReceiver<Self> &sender, ReplicaId replica_id,
        Write write)
    {
        sendMessage(sender, config.replica_address_list[replica_id], write);
    }

    void sendMessageToAll(const TransportReceiver<Self> &sender, Write write)
    {
        for (auto address : config.replica_address_list) {
            if (address != sender.address) {
                sendMessage(sender, address, write);
            }
        }
    }

    virtual void
    sendMessageToMulticast(const TransportReceiver<Self> &sender, Write write)
    {
        sendMessage(sender, config.multicast_address, write);
    }
};

} // namespace oscar