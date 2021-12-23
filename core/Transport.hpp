#pragma once
#include <chrono>
#include <functional>
#include <vector>

#include "core/Config.hpp"
#include "core/Type.hpp"

namespace oscar
{
/*! A struct required by C++, so `Transport`'s subclass can define essentials
ahead of instantiating base class.

Each specialization must include:
+ Associated type `Address`. It is assumed to be suitable for value semantic.
+ Compile-time constant `std::size_t` expression `BUFFER_SIZE`, the maximum
  length of serialized tx message. Sender's message length is bounded by this so
  transport do not need to provide dynamic allocated buffer.
*/
template <typename Transport> struct TransportMeta {
    // would be better to use concept?

    // using Address = ...;
    // static constexpr std::size_t BUFFER_SIZE = ...;

    // TODO N_CONCURRENT_MAX
};

template <typename Transport> class TransportReceiver;

/*! General supporting runtime for `TransportReceiver`, the sending half of the
actor model (and more).

The base class takes curiously recurring template pattern. The subclass
`Transport` is passed as template argument to `Transport`, as well as `Config`,
`TransportReceiver`, etc.

Besides of all interfaces defined here, a `Transport` implementation probably
wants to provide a `run` method to start main loop.
*/
template <typename Self> class Transport
{
protected:
    //! Construct base class with global configuration.
    //!
    Transport(const Config<Self> &config) : config(config) {}

public:
    virtual ~Transport() {}

    //! Value-semantic address type. Possible choices include plain string,
    //! `socketaddr_in`, or more customized ones.
    using Address = typename TransportMeta<Self>::Address;

    //! Dynamically allocate free address, mainly for construct `Client`.
    //!
    virtual Address allocateAddress() = 0;

    //! Configuration for `Transport` instance, along with all registered
    //! receivers.
    const Config<Self> &config;

    //! Register a receiver. The caller ensures that receiver's reference will
    //! be valid as long as message get received on receiver's address, which
    //! probably happens through the rest of `Transport`'s lifetime.
    virtual void registerReceiver(TransportReceiver<Self> &receiver) = 0;
    //! Register a receiver that listens to multicast messages. Transport
    //! implementation may require receiver to call `registerReceiver` first.
    virtual void
    registerMulticastReceiver(TransportReceiver<Self> &receiver) = 0;

    //! General task closure. Receiver can keep things inside it alive between
    //! working steps.
    using Callback = std::function<void()>;

    //! Schedule a one-time timeout. `callback` will be called after `delay`,
    //! cannot be canceled.
    //!
    //! Unlike receiving message, timeout is non-direction, i.e., does not have
    //! a `receiver`. The behavior depends on `callback`'s capturing.
    //!
    //! Timeout's `callback` is always executed sequentially.
    virtual void
    scheduleTimeout(std::chrono::microseconds delay, Callback callback) = 0;

    //! Schedule a sequential task. Sequential task is promised to be executed
    //! one by one without concurrency, so different sequential tasks can share
    //! states without locking. However, it is not guaranteed that all
    //! sequential tasks will be executed by the same thread.
    //!
    //! A default implementation is provided mainly for implying that timeout is
    //! sequential task by default, so there is no need for calling
    //! `scheduleSequential` inside timeout callback.
    virtual void scheduleSequential(Callback callback)
    {
        scheduleTimeout(std::chrono::microseconds(0), callback);
    }

    //! Schedule a concurrent task. Multiple concurrent tasks may be executed
    //! with temporal overlapping, so they would better to be stateless.
    virtual void scheduleConcurrent(Callback callback) = 0;

    //! Every concurrent task get assigned to a concurrent id, which can be
    //! queried by calling `getConcurrentId` during execution. Concurrent id is
    //! unsigned and bounded.
    //!
    //! Transport implementation promise that all concurrent tasks that get
    //! assigned to the same concurrent id will be sequentially executed. So it
    //! is safe to reuse pre-allocated memory "slot" among those tasks.
    //!
    //! Transport may set concurrent id to `-1` for sequential tasks, but it is
    //! encouraged to determine sequential tasks by themselves.
    virtual int getConcurrentId() const = 0;

    //! The size of buffer of tx message. May be helpful in assertion.
    //!
    static constexpr std::size_t BUFFER_SIZE = TransportMeta<Self>::BUFFER_SIZE;

    //! Argument for `sendMessage*` methods. The lambda should in-place
    //! serialize message into `buffer`, and return message length. The returned
    //! value must not exceed `BUFFER_SIZE`.
    using Write = std::function<std::size_t(Buffer<BUFFER_SIZE> &buffer)>;

    //! Send a message. The first argument is probably `*this`.
    //!
    //! Transport implementation promise not to access `write` after this method
    //! return, so the closure can capture references with no worry.
    virtual void sendMessage(
        const TransportReceiver<Self> &sender, const Address &dest,
        Write write) = 0;

    void sendMessageToReplica(
        const TransportReceiver<Self> &sender, ReplicaId replica_id,
        Write write)
    {
        sendMessage(sender, config.replica_address_list[replica_id], write);
    }

    //! The default implementation is mainly for illustration purpose. Transport
    //! should provide a more efficient version that only serialize once.
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