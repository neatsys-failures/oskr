#pragma once
#include "core/Transport.hpp"
#include "core/Type.hpp"
#include "core/Utility.hpp"

namespace oskr
{
/*! @brief A conventional receiver abstraction mostly for some kind of backward
compatibility.

With the new design of `TransportTrait`, transport is not longer requiring to
register with `TransportReceiver &` but instead with general closure. As the
result, protocol implementation is not required to inherit from
`TransportReceiver`. This base class still exists to provide an adapted and
simpler interface for some straightforward protocol implementation.

* @note If subclass want to access protected `transport` member, it need to
```
using TransportReceiver<Transport>::transport;
```
first.
*/
template <TransportTrait Transport> class TransportReceiver
{
protected:
    Transport &transport;

public:
    const typename Transport::Address address;

    TransportReceiver(
        Transport &transport, typename Transport::Address address) :
        transport(transport),
        address(address)
    {
        transport.registerReceiver(address, [&](auto &remote, auto desc) {
            transport.spawn([&, desc = std::move(desc)]() mutable {
                receiveMessage(remote, RxSpan(desc.data(), desc.size()));
            });
        });
    }
    virtual ~TransportReceiver() {}

    virtual void
    receiveMessage(const typename Transport::Address &remote, RxSpan span) = 0;
};

} // namespace oskr