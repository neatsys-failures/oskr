#pragma once
#include <map>
#include <optional>
#include <set>

#include "core/Foundation.hpp"

namespace oskr
{
struct RequestMessage {
    ClientId client_id;
    RequestNumber request_number;
    Data op;

    template <typename S> void serialize(S &s)
    {
        s(client_id, request_number, op);
    }
};

struct ReplyMessage {
    RequestNumber request_number;
    Data result;
    ViewNumber view_number;
    ReplicaId replica_id;

    template <typename S> void serialize(S &s)
    {
        s(request_number, result, view_number, replica_id);
    }
};

template <typename Protocol = void> struct ClientSetting {
};
template <> struct ClientSetting<void> {
    enum Strategy {
        UNSPECIFIED,
        ALL,
        PRIMARY_FIRST,
    };
};

template <TransportTrait Transport, typename Protocol>
class BasicClient : public Client<Transport>
{
public:
private:
    using TransportReceiver<Transport>::transport;
    using Client<Transport>::client_id;

    RequestNumber request_number;
    ViewNumber view_number;

    struct PendingRequest {
        RequestNumber request_number;
        Data op;
        std::map<Data, std::set<ReplicaId>> result_table;
        typename BasicClient::InvokeCallback callback;
    };
    std::optional<PendingRequest> pending;

public:
    BasicClient(Transport &transport) : Client<Transport>(transport)
    {
        request_number = 0;
        view_number = 0;
    }

    void
    invoke(Data op, typename BasicClient::InvokeCallback callback) override;

    void
    receiveMessage(const typename Transport::Address &, RxSpan span) override
    {
        ReplyMessage reply;
        deserializeReplyMessage(span, reply);
        handleReply(reply);
    }

    virtual std::size_t serializeRequestMessage(
        TxSpan<Transport::BUFFER_SIZE> buffer,
        const typename ClientSetting<Protocol>::ReplicaMessage &request)
    {
        return bitserySerialize(buffer, request);
    }

    virtual void deserializeReplyMessage(RxSpan span, ReplyMessage &reply)
    {
        bitseryDeserialize(span, reply);
    }

private:
    void sendRequest(bool resend);
    void handleReply(const ReplyMessage &reply);
};

template <TransportTrait Transport, typename ReplicaMessage>
void BasicClient<Transport, ReplicaMessage>::invoke(
    Data op, typename BasicClient::InvokeCallback callback)
{
    if (pending) {
        panic("Invoke pending client");
    }

    request_number += 1;
    pending = PendingRequest{request_number, op, {}, std::move(callback)};
    sendRequest(false);
}

template <TransportTrait Transport, typename Protocol>
void BasicClient<Transport, Protocol>::sendRequest(bool resend)
{
    RequestMessage request;
    request.client_id = client_id;
    request.request_number = pending->request_number;
    request.op = pending->op;
    auto write = [&](auto buffer) {
        return serializeRequestMessage(
            buffer, typename ClientSetting<Protocol>::ReplicaMessage(request));
    };
    auto send_to_primary = [&] {
        transport.sendMessageToReplica(
            *this, transport.config.primaryId(view_number), write);
    };
    auto send_to_all = [&] { transport.sendMessageToAll(*this, write); };

    switch (ClientSetting<Protocol>::STRATEGY) {
    case ClientSetting<>::Strategy::ALL:
        send_to_all();
        break;
    case ClientSetting<>::Strategy::PRIMARY_FIRST:
        if (resend) {
            send_to_all();
        } else {
            send_to_primary();
        }
        break;
    default:
        panic("Unreachable");
    }

    transport.spawn(
        ClientSetting<Protocol>::RESEND_INTERVAL,
        [&, current_number = request_number] {
            if (!pending || request_number != current_number) {
                return;
            }
            warn("Resend: request number = {}", request_number);
            sendRequest(true);
        });
}

template <TransportTrait Transport, typename Protocol>
void BasicClient<Transport, Protocol>::handleReply(const ReplyMessage &reply)
{
    if (!pending) {
        return;
    }
    if (pending->request_number != reply.request_number) {
        return;
    }

    if (reply.view_number > view_number) {
        view_number = reply.view_number;
    }

    if (ClientSetting<Protocol>::N_MATCHED > 1) {
        pending->result_table[reply.result].insert(reply.replica_id);
        if (pending->result_table.at(reply.result).size() <
            ClientSetting<Protocol>::N_MATCHED) {
            return;
        }
    }

    auto callback = std::move(pending->callback);
    pending.reset();
    callback(reply.result);
}

} // namespace oskr