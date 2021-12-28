#pragma once
#include <chrono>
#include <functional>
#include <vector>

#include "core/Config.hpp"
#include "core/Type.hpp"

namespace oskr
{
template <typename Self> struct TransportMeta {
};

template <typename Self> class TransportBase
{
public:
    using Address = typename TransportMeta<Self>::Address;
    using Desc = typename TransportMeta<Self>::Desc;
    static constexpr std::size_t BUFFER_SIZE = TransportMeta<Self>::BUFFER_SIZE;

    using Receiver =
        std::function<void(const Address &remote, Desc descriptor)>;
    using Callback = std::function<void()>;
    using Write = std::function<std::size_t(TxSpan<BUFFER_SIZE> buffer)>;

    template <typename Sender>
    void sendMessageToReplica(
        const Sender &sender, ReplicaId replica_id, Write write)
    {
        Self &transport = static_cast<Self &>(*this);
        transport.sendMessage(
            sender, transport.config.replica_address_list[replica_id], write);
    }

    template <typename Sender>
    void sendMessageToAll(const Sender &sender, Write write)
    {
        Self &transport = static_cast<Self &>(*this);
        for (auto dest : transport.config.replica_address_list) {
            if (dest != sender.address) {
                transport.sendMessage(sender, dest, write);
            }
        }
    }

    template <typename Sender>
    void sendMessageToMulticast(const Sender &sender, Write write)
    {
        Self &transport = static_cast<Self &>(*this);
        transport.sendMessage(
            sender, transport.config.multicast_address, write);
    }
};

} // namespace oskr