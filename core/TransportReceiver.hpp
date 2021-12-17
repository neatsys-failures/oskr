#pragma once

namespace oscar
{
template <typename Transport> class TransportReceiver
{
public:
    const typename Transport::Address address;
    TransportReceiver(typename Transport::Address address) : address(address) {}

    virtual void receiveMessage(
        const typename Transport::Address &remote,
        const typename Transport::Message &message) = 0;

    // the codebase assume at most one multicast address present
    virtual void receiveMulticastMessage(
        const typename Transport::Address &remote,
        const typename Transport::Message &message)
    {
        receiveMessage(remote, message);
    }
};

} // namespace oscar