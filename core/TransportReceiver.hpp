#pragma once
#include "core/Type.hpp"
#include "core/Utility.hpp"

namespace oscar
{
/*! @brief The base class for packet-listeners, i.e., all participants of
protocols. The receiving half of the actor model.

Each instance of `TransportReceiver` has a static address throughout its
lifetime. It also can only work with one specific `Transport` type.

After registered to a `Transport` instance, virtual method `receiveMessage` will
be called whenever messages arrives receiver's address.

The `TransportReceiver` itself does not make any assumption on how to process
received message, but a subclass will probably want to call `Transport` methods
to send message and schedule timeout. So it is conventional to ask for a
reference of `Transport` instance when constructing subclass. Because the
`Transport` base class follows CRTP, `Transport &` is basically equals to
`oscar::Transport<Transport> &` in every situation.
*/
template <typename Transport> class TransportReceiver
{
public:
    //! Static receiving address of the receiver. Must be unique across the
    //! whole system.
    const typename Transport::Address address;

    //! Construct receiver base class with address.
    //!
    TransportReceiver(typename Transport::Address address) : address(address) {}
    virtual ~TransportReceiver() {}

    //! Handle received raw message. The underlying memory that backs `span`
    //! will go out of lifetime after this method returns, so receiver need to
    //! own everything necessary for later processing.
    virtual void
    receiveMessage(const typename Transport::Address &remote, Span span)
    {
        panic("Unimplemented");
    }

    virtual void
    glanceMessage(const typename Transport::Address &remote, Span span)
    {
        // transport.spawn(
        //     bind(&TransportReceiver::receiverMessage, this, 
        //         remote, move(span)))
    }
};

/*! Although theoretically a multicast receiver is not required to be
addressable if it does not listen to any unicast, the situation not present for
known protocols (or can be easily workaround-ed by assigning a dummy address),
and we try to avoid multi-inheritance here.
*/
// the codebase assume at most one multicast address present
template <typename Transport>
class TransportMulticastReceiver : public TransportReceiver<Transport>
{
    // TODO define methods
};

} // namespace oscar