#pragma once
#include <chrono>
#include <functional>
#include <optional>

#include "core/Foundation.hpp"

namespace oskr
{
template <TransportTrait Transport> class StatefulTimeout
{
    std::optional<int> current_id;
    int timeout_id;

    Transport &transport;
    Fn<void()> callback;
    std::chrono::microseconds delay;

public:
    // TODO design a reasonable default constructor if necessary later
    StatefulTimeout(
        Transport &transport, std::chrono::microseconds delay,
        Fn<void()> callback) :
        transport(transport)
    {
        timeout_id = 0;
        this->callback = callback;
        this->delay = delay;
    }

    ~StatefulTimeout() { disable(); }

    void reset()
    {
        timeout_id += 1;
        current_id = timeout_id;
        transport.spawn(delay, [&, current_id = current_id] {
            if (this->current_id != current_id) {
                return;
            }
            disable();
            callback();
        });
    }

    void enable()
    {
        if (!current_id) {
            reset();
        }
    }

    void disable() { current_id = std::nullopt; }
};

} // namespace oskr