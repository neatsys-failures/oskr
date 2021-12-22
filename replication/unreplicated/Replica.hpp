#pragma once
#include "common/ClientTable.hpp"
#include "common/ListLog.hpp"
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
    Log<>::List &log;

    static constexpr auto bitserySerialize =
        oscar::bitserySerialize<Buffer<Transport::BUFFER_SIZE>, ReplyMessage>;

public:
    Replica(Transport &transport, Log<>::List &log) :
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
    auto send_reply = [this](auto &remote, const ReplyMessage &reply) {
        transport.sendMessage(
            *this, remote, std::bind(bitserySerialize, _1, reply));
    };

    if (auto apply = client_table.checkShortcut(
            remote, request.client_id, request.request_number)) {
        apply(send_reply);
        return;
    }

    op_number += 1;
    ListLog::Block block{
        {Log<>::Entry{request.client_id, request.request_number, request.op}},
        1};
    log.prepare(op_number, block);
    log.commit(
        op_number,
        [&](ClientId client_id, RequestNumber request_number, Data result) {
            ReplyMessage reply;
            reply.request_number = request_number;
            reply.result = result;
            // we don't need to set other thing
            if (auto apply = client_table.update(client_id, reply)) {
                apply(send_reply);
            }
        });
}

} // namespace oscar::unreplicated
