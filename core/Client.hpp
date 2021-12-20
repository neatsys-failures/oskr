#pragma once
#include <functional>
#include <random>

#include "core/TransportReceiver.hpp"

namespace oscar
{
template <typename Transport> class Client : public TransportReceiver<Transport>
{
protected:
    std::uint32_t client_id;

public:
    Client(Transport &transport)
        : TransportReceiver<Transport>(transport.allocateAddress())
    {
        std::random_device rand;
        std::default_random_engine engine(rand());
        client_id = std::uniform_int_distribution<std::uint32_t>()(engine);
    }

    using InvokeCallback = std::function<void(Data result)>;
    virtual void invoke(Data op, InvokeCallback callback) = 0;
};

} // namespace oscar