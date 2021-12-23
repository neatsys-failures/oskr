#pragma once
#include <chrono>
#include <functional>
#include <optional>

#include "core/Foundation.hpp"

namespace oscar
{
template <typename Transport> class StatefulTimeout
{
    std::optional<int> current_id;
    int timeout_id;

    Transport &transport;
    std::function<void()> callback;
    std::chrono::microseconds delay;

public:
    // TODO design a reasonable default constructor if necessary later
    StatefulTimeout(
        Transport &transport, std::chrono::microseconds delay,
        std::function<void()> callback) :
        transport(transport)
    {
        timeout_id = 0;
        this->callback = callback;
        this->delay = delay;
    }

    void reset()
    {
        timeout_id += 1;
        current_id = timeout_id;
        transport.scheduleTimeout(delay, [this, current_id = this->current_id] {
            if (this->current_id != current_id) {
                return;
            }
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

} // namespace oscar