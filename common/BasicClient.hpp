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

template <TransportTrait Transport, typename ReplicaMessage>
class BasicClient : public Client<Transport>
{
public:
    struct Config {
        enum Strategy {
            UNSPECIFIED,
            ALL,
            PRIMARY_FIRST,
        } strategy;
        std::chrono::microseconds resend_interval;
        std::size_t n_matched;
    };

private:
    using TransportReceiver<Transport>::transport;
    Config config;

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
    BasicClient(Transport &transport, Config config) :
        Client<Transport>(transport)
    {
        this->config = config;
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
        TxSpan<Transport::BUFFER_SIZE> buffer, const ReplicaMessage &request)
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
    pending = PendingRequest{request_number, op, {}, callback};
    sendRequest(false);
}

template <TransportTrait Transport, typename ReplicaMessage>
void BasicClient<Transport, ReplicaMessage>::sendRequest(bool resend)
{
    RequestMessage request;
    request.client_id = client_id;
    request.request_number = pending->request_number;
    request.op = pending->op;
    auto write = [&](auto buffer) {
        return serializeRequestMessage(buffer, ReplicaMessage(request));
    };
    auto send_to_primary = [&] {
        transport.sendMessageToReplica(
            *this, transport.config.primaryId(view_number), write);
    };
    auto send_to_all = [&] { transport.sendMessageToAll(*this, write); };

    switch (config.strategy) {
    case Config::Strategy::ALL:
        send_to_all();
        break;
    case Config::Strategy::PRIMARY_FIRST:
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
        config.resend_interval, [&, current_number = request_number] {
            if (!pending || request_number != current_number) {
                return;
            }
            warn("Resend: request number = {}", request_number);
            sendRequest(true);
        });
}

template <TransportTrait Transport, typename ReplicaMessage>
void BasicClient<Transport, ReplicaMessage>::handleReply(
    const ReplyMessage &reply)
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

    if (config.n_matched > 1) {
        pending->result_table[reply.result].insert(reply.replica_id);
        if (pending->result_table.at(reply.result).size() < config.n_matched) {
            return;
        }
    }

    auto callback = pending->callback;
    pending.reset();
    callback(reply.result);
}

} // namespace oskr