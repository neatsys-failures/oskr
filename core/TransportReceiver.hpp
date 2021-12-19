#pragma once
#include "core/Type.hpp"

namespace oscar
{
template <typename Transport> class TransportReceiver
{
public:
    const typename Transport::Address address;
    TransportReceiver(typename Transport::Address address) : address(address) {}

    virtual void receiveMessage(
        const typename Transport::Address &remote, const Span &buffer) = 0;

    // the codebase assume at most one multicast address present
    virtual void receiveMulticastMessage(
        const typename Transport::Address &remote, const Span &buffer)
    {
        receiveMessage(remote, buffer);
    }
};

} // namespace oscar