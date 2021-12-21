#pragma once
#include "common/ClientTable.hpp"
#include "core/Foundation.hpp"
#include "replication/unreplicated/Message.hpp"

namespace oscar::unreplicated
{
// work with:
// BasicClient<_, ReplicaMessage>(_, {Strategy::PRIMARY_FIRST, 1000ms, 1})

template <typename Transport>
class Replica : public TransportReceiver<Transport>
{
    Transport &transport;

    OpNumber op_number;
    ClientTable<Transport, ReplyMessage> client_table;
    Log &log;

    static constexpr auto bitserySerialize =
        oscar::bitserySerialize<typename Transport::Buffer, ReplyMessage>;

public:
    Replica(Transport &transport, Log &log) :
        TransportReceiver<Transport>(transport.config.replica_address_list[0]),
        transport(transport), log(log)
    {
        op_number = 0;
    }

    void receiveMessage(
        const typename Transport::Address &remote, const Span &span) override
    {
        using namespace std::placeholders;

        ReplicaMessage message;
        bitseryDeserialize(span, message);
        transport.scheduleSequential([this, remote, message] {
            std::visit(std::bind(&Replica::handle, this, remote, _1), message);
        });
    }

    void handle(
        const typename Transport::Address &remote,
        const RequestMessage &request);
};

template <typename Transport>
void Replica<Transport>::handle(
    const typename Transport::Address &remote, const RequestMessage &request)
{
    using std::placeholders::_1;

    if (auto shortcut = client_table.checkShortcut(
            remote, request.client_id, request.request_number)) {
        shortcut([this, remote](const ReplyMessage &reply) {
            transport.sendMessage(
                *this, remote, std::bind(bitserySerialize, _1, reply));
        });
        return;
    }

    //
}

} // namespace oscar::unreplicated
