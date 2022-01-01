#pragma once
#include "common/ClientTable.hpp"
#include "common/Quorum.hpp"
#include "common/StatefulTimeout.hpp"
#include "core/Foundation.hpp"
#include "replication/vr/Message.hpp"

namespace oskr::vr
{
template <TransportTrait Transport>
class Replica : public TransportReceiver<Transport>
{
    using TransportReceiver<Transport>::transport;
    ReplicaId replica_id;
    int batch_size;

    enum Status {
        normal,
        view_change,
    } status;
    ViewNumber view_number;
    OpNumber op_number, commit_number;

    Log<>::List::Block batch;
    ClientTable<Transport, ReplyMessage> client_table;
    Log<>::List &log;
    Quorum<OpNumber, PrepareOkMessage> prepare_ok_set;
    Quorum<ViewNumber, StartViewChangeMessage> start_view_change_set;
    Quorum<ViewNumber, DoViewChangeMessage> do_view_change_set;

    StatefulTimeout<Transport> idle_commit_timeout;
    StatefulTimeout<Transport> view_change_timeout;

public:
    Replica(
        Transport &transport, Log<>::List &log, ReplicaId replica_id,
        int batch_size) :
        TransportReceiver<Transport>(
            transport, transport.config.replica_address_list[replica_id]),
        log(log), prepare_ok_set(transport.config.n_fault),
        start_view_change_set(transport.config.n_fault),
        do_view_change_set(transport.config.n_fault + 1),
        idle_commit_timeout(
            transport, 200ms,
            [&] {
                rinfo("idle commit timeout");
                sendCommit();
            }),
        view_change_timeout(transport, 500ms, [&] {
            rwarn("view change timeout");
            startViewChange(view_number + 1);
        })
    {
        if (batch_size > Log<>::block_size) {
            panic("Batch size too large");
        }

        this->replica_id = replica_id;
        this->batch_size = batch_size;
        status = Status::normal;
        view_number = 0;
        op_number = commit_number = 0;
        batch.n_entry = 0;

        if (isPrimary()) {
            idle_commit_timeout.enable();
        } else {
            view_change_timeout.enable();
        }
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

    // I prefer the way to explicit convert `message` into `ReplicaMessage`
    // A simpler way maybe add a typename `OutMessage = ReplicaMessage` here and
    // do conversion inside
    template <typename Message>
    static typename Transport::Write write(const Message &message)
    {
        return [&](auto tx_span) {
            return bitserySerialize<Message, Transport::buffer_size>(
                tx_span, message);
        };
    }

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
    void handle(
        const typename Transport::Address &remote,
        const StartViewChangeMessage &start_view_change);
    void handle(
        const typename Transport::Address &remote,
        const DoViewChangeMessage &do_view_change);
    void handle(
        const typename Transport::Address &remote,
        const StartViewMessage &start_view);
    template <typename M> void handle(const typename Transport::Address &, M &)
    {
        rpanic("unreachable");
    }

    void sendReply(
        const typename Transport::Address &remote, const ReplyMessage &reply);
    void sendCommit();
    void sendDoViewChange();

    void closeBatch();
    void commitUpTo(OpNumber op_number);
    void startViewChange(ViewNumber start_view);
    void startView(const std::map<ReplicaId, DoViewChangeMessage> &quorum);
    void enterView(const StartViewMessage &start_view);
};

template <TransportTrait Transport>
void Replica<Transport>::handle(
    const typename Transport::Address &remote, const RequestMessage &request)
{
    if (status != Status::normal) {
        return;
    }

    if (auto apply = client_table.check(
            remote, request.client_id, request.request_number)) {
        apply(std::bind(&Replica::sendReply, this, _1, _2));
        return;
    }

    if (isPrimary()) {
        batch.entry_buffer[batch.n_entry] =
            Log<>::Entry{request.client_id, request.request_number, request.op};
        batch.n_entry += 1;
        if (batch.n_entry >= batch_size) {
            closeBatch();
        }
    }
}

template <TransportTrait Transport> void Replica<Transport>::closeBatch()
{
    if (status != Status::normal || !isPrimary()) {
        panic("unreachable");
    }

    op_number += 1;
    log.prepare(op_number, batch);
    PrepareMessage prepare;
    prepare.view_number = view_number;
    prepare.op_number = op_number;
    prepare.commit_number = commit_number;
    prepare.block = batch;

    batch.n_entry = 0;

    transport.sendMessageToAll(*this, write(ReplicaMessage(prepare)));
    idle_commit_timeout.reset();

    if (prepare_ok_set.checkForQuorum(op_number)) {
        commitUpTo(op_number);
    }
}

template <TransportTrait Transport>
void Replica<Transport>::handle(
    const typename Transport::Address &, const PrepareMessage &prepare)
{
    if (status != Status::normal || view_number > prepare.view_number) {
        return;
    }

    if (view_number < prepare.view_number) {
        rpanic("todo");
    }

    if (isPrimary()) {
        rpanic("unreachable");
    }

    view_change_timeout.reset();

    if (prepare.op_number <= op_number) {
        // TODO resend PrepareOk
        return;
    }
    if (prepare.op_number != op_number + 1) {
        rpanic("todo");
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
        write(ReplicaMessage(prepare_ok)));

    if (prepare.commit_number > commit_number) {
        commitUpTo(prepare.commit_number);
    }
}

template <TransportTrait Transport>
void Replica<Transport>::handle(
    const typename Transport::Address &, const PrepareOkMessage &prepare_ok)
{
    if (status != Status::normal || prepare_ok.view_number < view_number) {
        return;
    }
    if (prepare_ok.view_number > view_number) {
        rpanic("todo");
    }
    if (!isPrimary()) {
        rpanic("unreachable");
    }

    if (prepare_ok.op_number <= commit_number) {
        return;
    }

    if (prepare_ok_set.addAndCheckForQuorum(
            prepare_ok.op_number, prepare_ok.replica_id, prepare_ok)) {
        commitUpTo(prepare_ok.op_number);
    }
}

template <TransportTrait Transport>
void Replica<Transport>::commitUpTo(OpNumber op_number)
{
    for (OpNumber i = commit_number + 1; i <= op_number; i += 1) {
        log.commit(
            i,
            [&](ClientId client_id, RequestNumber request_number, Data result) {
                ReplyMessage reply;
                reply.request_number = request_number;
                reply.result = result;
                reply.view_number = view_number;
                client_table.update(client_id, request_number, reply)(
                    std::bind(&Replica::sendReply, this, _1, _2));
            });
    }
    commit_number = op_number;
}

template <TransportTrait Transport>
void Replica<Transport>::handle(
    const typename Transport::Address &, const CommitMessage &commit)
{
    if (status != Status::normal || commit.view_number < view_number) {
        return;
    }

    if (commit.view_number > view_number) {
        rpanic("todo");
    }

    view_change_timeout.reset();

    if (commit.commit_number > commit_number) {
        commitUpTo(commit.commit_number);
    }
}

template <TransportTrait Transport>
void Replica<Transport>::startViewChange(ViewNumber start_view)
{
    if (isPrimary()) {
        panic("unreachable");
    }

    status = Status::view_change;
    view_number = start_view;
    rinfo("start view change: view number = {}", view_number);

    view_change_timeout.reset(); // start counting for next primary

    StartViewChangeMessage start_view_change;
    start_view_change.view_number = view_number;
    start_view_change.replica_id = replica_id;
    transport.sendMessageToAll(*this, write(ReplicaMessage(start_view_change)));
}

template <TransportTrait Transport>
void Replica<Transport>::handle(
    const typename Transport::Address &,
    const StartViewChangeMessage &start_view_change)
{
    if (start_view_change.view_number < view_number) {
        return;
    }
    if (start_view_change.view_number > view_number) {
        startViewChange(start_view_change.view_number);
    }
    // now view number == start view change message's view number

    if (start_view_change_set.addAndCheckForQuorum(
            view_number, start_view_change.replica_id, start_view_change)) {
        // TODO prevent duplicate send
        sendDoViewChange();
    }
}

template <TransportTrait Transport>
void Replica<Transport>::handle(
    const typename Transport::Address &,
    const DoViewChangeMessage &do_view_change)
{
    if (do_view_change.view_number < view_number) {
        return;
    }
    if (do_view_change.view_number > view_number) {
        startViewChange(do_view_change.view_number);
    }
    if (!isPrimary()) {
        panic("unreachable");
    }

    if (status != Status::view_change) {
        // TODO resend for late backup
        return;
    }

    if (auto quorum = do_view_change_set.addAndCheckForQuorum(
            view_number, do_view_change.replica_id, do_view_change)) {
        startView(*quorum);
    }
}

template <TransportTrait Transport>
void Replica<Transport>::startView(
    const std::map<ReplicaId, DoViewChangeMessage> &quorum)
{
    if (!isPrimary()) {
        panic("unreachable");
    }

    OpNumber max_commit = commit_number;
    for (auto pair : quorum) {
        auto &do_view_change = pair.second;
        if (do_view_change.op_number > op_number) {
            return; // give up
        }
        if (do_view_change.commit_number > max_commit) {
            max_commit = do_view_change.commit_number;
        }
    }

    StartViewMessage start_view;
    start_view.view_number = view_number;
    start_view.log = ZeroLog();
    start_view.op_number = op_number;
    start_view.commit_number = commit_number;
    transport.sendMessageToAll(*this, write(ReplicaMessage(start_view)));

    enterView(start_view);
}

template <TransportTrait Transport>
void Replica<Transport>::enterView(const StartViewMessage &start_view)
{
    view_number = start_view.view_number;
    rinfo("enter view: view number = {}", view_number);

    status = Status::normal;
    batch.n_entry = 0;
    prepare_ok_set.clear(); // is it necessary?
    if (!isPrimary()) {
        view_change_timeout.reset();
    } else {
        view_change_timeout.disable();
        idle_commit_timeout.enable();
    }

    if (op_number < start_view.op_number) {
        panic("todo"); // state transfer;
    }

    if (start_view.commit_number > commit_number) {
        commitUpTo(start_view.commit_number);
    }
}

template <TransportTrait Transport>
void Replica<Transport>::handle(
    const typename Transport::Address &, const StartViewMessage &start_view)
{
    if (start_view.view_number < view_number) {
        return;
    }
    if (start_view.view_number == view_number &&
        status != Status::view_change) {
        return;
    }

    enterView(start_view);
}

template <TransportTrait Transport>
void Replica<Transport>::sendReply(
    const typename Transport::Address &remote, const ReplyMessage &reply)
{
    if (!isPrimary()) {
        return;
    }
    transport.sendMessage(*this, remote, write(reply));
}

template <TransportTrait Transport> void Replica<Transport>::sendCommit()
{
    if (!isPrimary()) {
        panic("unreachable");
    }
    CommitMessage commit;
    commit.view_number = view_number;
    commit.commit_number = commit_number;
    transport.sendMessageToAll(*this, write(ReplicaMessage(commit)));

    idle_commit_timeout.reset();
}

template <TransportTrait Transport> void Replica<Transport>::sendDoViewChange()
{
    rinfo("do view change: view number = {}", view_number);
    DoViewChangeMessage do_view_change;
    do_view_change.view_number = view_number;
    do_view_change.log = ZeroLog();
    // do_view_change.latest_normal
    do_view_change.op_number = op_number;
    do_view_change.commit_number = commit_number;
    do_view_change.replica_id = replica_id;

    // is this good design?
    if (!isPrimary()) {
        transport.sendMessageToReplica(
            *this, transport.config.primaryId(view_number),
            write(ReplicaMessage(do_view_change)));
    } else {
        if (auto quorum = do_view_change_set.addAndCheckForQuorum(
                view_number, replica_id, do_view_change)) {
            startView(*quorum);
        }
    }
}

} // namespace oskr::vr