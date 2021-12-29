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

template <typename Transport> struct SenderArchetype {
    const typename Transport::Address address;
};

/*! Transport concept. The representation of the actor model used by this
project.

> Despite the name, `TransportReceiver` is not a necessary part of the
abstraction, and transport does not depend on it.
*/
template <typename T>
concept TransportTrait = requires
{
    typename TransportMeta<T>::Address;
    typename TransportMeta<T>::Desc;
    typename std::integral_constant<std::size_t, TransportMeta<T>::BUFFER_SIZE>;
}
&&std::derived_from<T, TransportBase<T>> &&requires(
    T transport, const typename T::Address address,
    typename T::Receiver receiver)
{
    transport.registerReceiver(address, receiver);
    // TODO multicast
}
&&requires(T transport, typename T::Callback callback)
{
    transport.spawn(callback);
}
&&requires(
    T transport, const SenderArchetype<T> &sender,
    const typename T::Address &dest, typename T::Write write)
{
    transport.sendMessage(sender, dest, write);
};

} // namespace oskr