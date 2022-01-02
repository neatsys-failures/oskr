#pragma once
#include <chrono>
#include <functional>
#include <optional>

#include "core/Foundation.hpp"

namespace oskr
{
/*! A handy wrapper around transport's `spawn` (timeout) method.

You can use `StatefulTimeout` to model those "meaningful" timeouts, e.g., the
timeout for sending/checking heartbeat message. You cannot use this when:
- The delay is variable.
- You need a one-time timeout whose callback is not reentrant, i.e., takes
non-copyable capture list.

* @note Although it is *stateful*, it is still a *one-time* timeout. If you want
to spawn periodical callback, e.g., resending until succeed, you need to call
`reset` manually inside the callback.
*/
template <TransportTrait Transport> class StatefulTimeout
{
    Transport &transport;
    Fn<void()> callback;
    std::chrono::microseconds delay;

    Fn<void()> cancel;

public:
    //! Construct a timeout. After `enable` for `delay`, the `callback` will be
    //! invoked.
    //!
    //! @note The `callback` will be re-invoked every time the condition meets,
    //! so it is a `Fn`, not a `FnOnce` as `spawn`'s argument.
    // TODO design a reasonable default constructor if necessary later
    StatefulTimeout(
        Transport &transport, std::chrono::microseconds delay,
        Fn<void()> callback) :
        transport(transport)
    {
        this->callback = callback;
        this->delay = delay;
    }

    //! Timeout will be disabled automatically on deconstruct.
    //!
    ~StatefulTimeout() { disable(); }

    //! Clear previous state. Timeout will be triggered after `delay` since
    //! calling whatever case.
    void reset()
    {
        disable();
        cancel = transport.spawn(delay, [&] {
            cancel = nullptr;
            callback();
        });
    }

    //! Similar to `reset`, only nothing will happen except the first calling,
    //! i.e. `delay` is not reset on the following callings.
    void enable()
    {
        if (!cancel) {
            reset();
        }
    }

    //! Make sure nothing will happen in the future whatever previous state.
    //!
    void disable()
    {
        if (cancel) {
            cancel();
        }
        cancel = nullptr;
    }
};

} // namespace oskr