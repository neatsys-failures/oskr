#pragma once
#include "common/ClientTable.hpp"
#include "common/ListLog.hpp"
#include "common/Quorum.hpp"
#include "common/StatefulTimeout.hpp"
#include "core/Foundation.hpp"
#include "replication/vr/Message.hpp"

namespace oskr::vr
{
// work with:
// BasicClient<_, ReplicaMessage>(_, {Strategy::PRIMARY_FIRST, 1000ms, 1})

template <TransportTrait Transport>
class Replica : public TransportReceiver<Transport>
{
    using TransportReceiver<Transport>::transport;
    ReplicaId replica_id;
    int batch_size;

    enum Status {
        NORMAL,
        VIEW_CHANGE,
    } status;
    ViewNumber view_number;
    OpNumber op_number, commit_number;

    Log<>::List::Block batch;
    ClientTable<Transport, ReplyMessage> client_table;
    Log<>::List &log;
    Quorum<OpNumber, PrepareOkMessage> prepare_set;

public:
    Replica(
        Transport &transport, Log<>::List &log, ReplicaId replica_id,
        int batch_size) :
        TransportReceiver<Transport>(
            transport, transport.config.replica_address_list[replica_id]),
        log(log), prepare_set(transport.config.n_fault)
    {
        if (batch_size > Log<>::BLOCK_SIZE) {
            panic("Batch size too large");
        }

        this->replica_id = replica_id;
        this->batch_size = batch_size;
        status = Status::NORMAL;
        view_number = 0;
        op_number = commit_number = 0;
        batch.n_entry = 0;
    }

    void receiveMessage(
        const typename Transport::Address &remote, RxSpan span) override
    {
        ReplicaMessage message;
        bitseryDeserialize(span, message);
        std::visit([&](const auto &m) { handle(remote, m); }, message);
    }

private:
    static constexpr auto _1 = std::placeholders::_1;
    static constexpr auto _2 = std::placeholders::_2;
    template <typename Message = ReplicaMessage>
    static constexpr auto bitserySerialize =
        oskr::bitserySerialize<Message, Transport::BUFFER_SIZE>;

    bool isPrimary() const
    {
        return transport.config.primaryId(view_number) == replica_id;
    }

    void handle(
        const typename Transport::Address &remote,
        const RequestMessage &request);
    void handle(
        const typename Transport::Address &remote,
        const PrepareMessage &prepare);
    void handle(
        const typename Transport::Address &remote,
        const PrepareOkMessage &prepare_ok);
    void handle(
        const typename Transport::Address &remote, const CommitMessage &commit);
    // void handle(
    //     const typename Transport::Address &remote,
    //     const StartViewChangeMessage &start_view_change);
    // void handle(
    //     const typename Transport::Address &remote,
    //     const DoViewChangeMessage &do_view_change);
    // void handle(
    //     const typename Transport::Address &remote,
    //     const StartViewMessage &start_view);
    template <typename M> void handle(const typename Transport::Address &, M &)
    {
        rpanic("Unreachable");
    }

    void
    send(const ReplyMessage &reply, const typename Transport::Address &remote);

    void closeBatch();
    void commitUpTo(OpNumber op_number);
};

template <TransportTrait Transport>
void Replica<Transport>::handle(
    const typename Transport::Address &remote, const RequestMessage &request)
{
    if (status != Status::NORMAL || !isPrimary()) {
        return;
    }

    if (auto apply = client_table.check(
            remote, request.client_id, request.request_number)) {
        apply(std::bind(&Replica::send, this, _2, _1));
        return;
    }

    batch.entry_buffer[batch.n_entry] =
        Log<>::Entry{request.client_id, request.request_number, request.op};
    batch.n_entry += 1;
    if (batch.n_entry >= batch_size) {
        closeBatch();
    }
}

template <TransportTrait Transport> void Replica<Transport>::closeBatch()
{
    op_number += 1;
    log.prepare(op_number, batch);
    PrepareMessage prepare;
    prepare.view_number = view_number;
    prepare.op_number = op_number;
    prepare.block = batch;
    transport.sendMessageToAll(
        *this, std::bind(bitserySerialize<>, _1, ReplicaMessage(prepare)));

    if (prepare_set.checkForQuorum(op_number)) {
        commitUpTo(op_number);
    }
}

template <TransportTrait Transport>
void Replica<Transport>::handle(
    const typename Transport::Address &, const PrepareMessage &prepare)
{
    if (status != Status::NORMAL || view_number > prepare.view_number) {
        return;
    }
    if (view_number < prepare.view_number) {
        rpanic("Todo");
    }

    if (isPrimary()) {
        rpanic("Unreachable");
    }

    if (prepare.op_number <= op_number) {
        // TODO resend PrepareOk
        return;
    }
    if (prepare.op_number != op_number + 1) {
        rpanic("Todo");
        return;
    }

    op_number += 1;
    log.prepare(op_number, prepare.block);
    for (int i = 0; i < prepare.block.n_entry; i += 1) {
        auto &entry = prepare.block.entry_buffer[i];
        client_table.update(entry.client_id, entry.request_number);
    }

    PrepareOkMessage prepare_ok;
    prepare_ok.view_number = view_number;
    prepare_ok.op_number = op_number;
    prepare_ok.replica_id = replica_id;
    transport.sendMessageToReplica(
        *this, transport.config.primaryId(view_number),
        std::bind(bitserySerialize<>, _1, ReplicaMessage(prepare_ok)));

    if (prepare.commit_number > commit_number) {
        commitUpTo(prepare.commit_number);
    }
}

template <TransportTrait Transport>
void Replica<Transport>::handle(
    const typename Transport::Address &, const PrepareOkMessage &prepare_ok)
{
    if (status != Status::NORMAL || prepare_ok.view_number < view_number) {
        return;
    }
    if (prepare_ok.view_number > view_number) {
        rpanic("Todo");
    }
    if (!isPrimary()) {
        rpanic("Unreachable");
    }

    if (prepare_ok.op_number <= commit_number) {
        return;
    }

    if (prepare_set.addAndCheckForQuorum(
            prepare_ok.op_number, prepare_ok.replica_id, prepare_ok)) {
        commitUpTo(prepare_ok.op_number);
    }
}

template <TransportTrait Transport>
void Replica<Transport>::commitUpTo(OpNumber op_number)
{
    for (OpNumber i = commit_number; i <= op_number; i += 1) {
        log.commit(
            i,
            [&](ClientId client_id, RequestNumber request_number, Data result) {
                ReplyMessage reply;
                reply.request_number = request_number;
                reply.result = result;
                reply.view_number = view_number;
                client_table.update(client_id, request_number, reply)(
                    std::bind(&Replica::send, this, _2, _1));
            });
    }
    commit_number = op_number;
}

template <TransportTrait Transport>
void Replica<Transport>::handle(
    const typename Transport::Address &, const CommitMessage &commit)
{
    if (status != Status::NORMAL || commit.view_number < view_number) {
        return;
    }
    if (commit.view_number > view_number) {
        rpanic("Todo");
    }
    if (commit.commit_number <= commit_number) {
        return;
    }
    commitUpTo(commit.commit_number);
}

template <TransportTrait Transport>
void Replica<Transport>::send(
    const ReplyMessage &reply, const typename Transport::Address &remote)
{
    transport.sendMessage(
        *this, remote, std::bind(bitserySerialize<ReplyMessage>, _1, reply));
}

} // namespace oskr::vr