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

/*! @brief Common complement of `TransportTrait` concept. Including re-export
and derivation of types and serveral default implementations. All methods are
not virtual be design, because the whole `Transport` is a compile-time
polymorphism.
*/
template <typename Self> struct TransportBase {
    using Address = typename TransportMeta<Self>::Address;
    using Desc = typename TransportMeta<Self>::Desc;
    static constexpr std::size_t BUFFER_SIZE = TransportMeta<Self>::BUFFER_SIZE;

    //! Receiver closure. Related states should be captured by needed.
    //!
    //! The closure will be called on every packet-receiving. The `remote`
    //! reference is alive as long as transport object is alive. The provided
    //! `descriptor` keeps the underlying message in scope. Although receiver is
    //! allowed to keep `descriptor` after returning from this callback,
    //! receiver should release `descriptor` as soon as possible, make sure to
    //! maintain a upper bound of number of kept `description` to prevent
    //! transport's descriptor get exhausted.
    //!
    //! @warning Receiver must put as lightest as possible processing logic in
    //! this callback, do only the most basic works like `spawn` or even nothing
    //! (e.g., drop the packet), and return quickly.
    //! @warning This is required because receiver is called from RX thread,
    //! which is unacceptable to be blocked by protocol logic. For certain
    //! transport (e.g., DPDK-based), if RX interval is too large, incoming
    //! packets will be dropped in network interface, and such situation will be
    //! considered as **fatal error**. Understand what you are doing in
    //! receiver.
    using Receiver = Fn<void(const Address &remote, Desc descriptor)>;
    using Callback = FnOnce<void()>;

    //! The common argument of `sendMessage*` methods, write message content
    //! into `buffer`, return written length, which should not be greater than
    //! `BUFFER_SIZE`.
    //!
    //! Transport promises that this callback will not be accessed any more
    //! after returning from `sendMessage*`. It is safe to capture local objects
    //! by reference.
    using Write = Fn<std::size_t(TxSpan<BUFFER_SIZE> buffer)>;

    template <typename Sender>
    void sendMessageToReplica(
        const Sender &sender, ReplicaId replica_id, Write write)
    {
        Self &transport = static_cast<Self &>(*this);
        transport.sendMessage(
            sender, transport.config.replica_address_list[replica_id], write);
    }

    //! Practical transport implementation should override this with a more
    //! efficient version that only call `write` once.
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

/*! @brief Transport concept. The representation of the actor model used by this
project.

* @note Despite the name, `TransportReceiver` is not a necessary part of the
abstraction, and transport does not depend on it.

Generally, `TransportTrait` can be considered as an extended version of original
spexpaxos's `Transport` interface. Because it is a concept rather than a
superclass, the name `Transport` is intended to be preserved as protocol
classes' template parameter.
*/
template <typename T>
concept TransportTrait =
    //! TransportMeta specialization.
    requires
{
    //! The address type used by transport implementation. The type should has
    //! value semantic, and avoid heap allocation.
    typename TransportMeta<T>::Address;
    typename TransportMeta<T>::Desc;
    typename std::integral_constant<std::size_t, TransportMeta<T>::BUFFER_SIZE>;
}
//! Subclassing `TransportBase`
&&std::derived_from<T, TransportBase<T>>
    //! Receiving interfaces
    &&requires(
        T transport, const typename T::Address address,
        typename T::Receiver receiver)
{
    transport.registerReceiver(address, std::move(receiver));
    // TODO multicast
}
//! Scheduling interfaces
&&requires(T transport, typename T::Callback callback)
{
    transport.spawn(std::move(callback));
}
//! Sending interfaces
&&requires(
    T transport, const SenderArchetype<T> &sender,
    const typename T::Address &dest, typename T::Write write)
{
    transport.sendMessage(sender, dest, std::move(write));
};

} // namespace oskr