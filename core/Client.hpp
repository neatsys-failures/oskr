#pragma once
#include <functional>
#include <random>

#include "core/TransportReceiver.hpp"
#include "core/Utility.hpp"

namespace oskr
{
/*! @brief Client model. Client is transport receiver which also provides method
to initiate something from external of the system.

Although not necessary, currently `Client` is derived from `TransportReceiver`,
because normally client does not care much (because it is too simple), and a
untyped client receiver is usually overkill.
*/
template <TransportTrait Transport>
class Client : public TransportReceiver<Transport>
{
protected:
    ClientId client_id;

public:
    Client(Transport &transport) :
        TransportReceiver<Transport>(transport, transport.allocateAddress())
    {
        client_id = std::uniform_int_distribution<ClientId>()(random_engine());
    }

    using InvokeCallback = FnOnce<void(Data result)>;
    virtual void invoke(Data op, InvokeCallback callback) = 0;
};

} // namespace oskr