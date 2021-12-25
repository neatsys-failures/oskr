#pragma once
#include <functional>
#include <random>

#include "core/TransportReceiver.hpp"
#include "core/Utility.hpp"

namespace oscar
{
template <typename Transport> class Client : public TransportReceiver<Transport>
{
protected:
    ClientId client_id;

public:
    Client(Transport &transport) :
        TransportReceiver<Transport>(transport.allocateAddress())
    {
        client_id = std::uniform_int_distribution<ClientId>()(random_engine());
    }

    using InvokeCallback = std::function<void(Data result)>;
    virtual void invoke(Data op, InvokeCallback callback) = 0;
};

} // namespace oscar