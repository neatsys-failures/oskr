#pragma once
#include <vector>

namespace oscar
{
template <typename Transport> class TransportReceiver
{
public:
    const typename Transport::Address address;

    virtual void ReceiveMessage(
        const typename Transport::Address &remote,
        const typename Transport::Message &message) = 0;

    // the codebase assume at most one multicast address present
    virtual void ReceiveMulticastMessage(
        const typename Transport::Address &remote,
        const typename Transport::Message &message)
    {
        ReceiveMessage(remote, message);
    }
};

} // namespace oscar