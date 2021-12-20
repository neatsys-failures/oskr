#pragma once
#include <map>
#include <optional>

#include "core/Foundation.hpp"

namespace oscar
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

template <typename Transport, typename ReplicaMessage>
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
    Transport &transport;
    Config config;

    RequestNumber request_number;
    ViewNumber view_number;

    struct PendingRequest {
        RequestNumber request_number;
        Data op;
        std::multimap<Data, ReplicaId> result_table;
        typename BasicClient::InvokeCallback callback;
    };
    std::optional<PendingRequest> pending;

public:
    BasicClient(Transport &transport, Config config) :
        Client<Transport>(transport), transport(transport)
    {
        this->config = config;
        request_number = 0;
        view_number = 0;
    }

    void
    invoke(Data op, typename BasicClient::InvokeCallback callback) override;

    void receiveMessage(
        const typename Transport::Address &remote, const Span &span) override
    {
        (void)remote;
        ReplyMessage reply;
        deserializeReplyMessage(span, reply);
        transport.scheduleSequential(
            std::bind(&BasicClient::handleReply, this, reply));
    }

    virtual std::size_t serializeRequestMessage(
        typename Transport::Buffer &buffer, const ReplicaMessage &request)
    {
        return bitserySerialize(buffer, request);
    }

    virtual void deserializeReplyMessage(const Span &span, ReplyMessage &reply)
    {
        bitseryDeserialize(span, reply);
    }

private:
    void sendRequest(bool resend);
    void handleReply(const ReplyMessage &reply);
};

template <typename Transport, typename ReplicaMessage>
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

template <typename Transport, typename ReplicaMessage>
void BasicClient<Transport, ReplicaMessage>::sendRequest(bool resend)
{
    RequestMessage request;
    request.client_id = this->client_id;
    request.request_number = pending->request_number;
    request.op = pending->op;
    auto write = [this, request](typename Transport::Buffer &buffer) {
        return serializeRequestMessage(buffer, ReplicaMessage(request));
    };
    auto send_to_primary = [this, write] {
        transport.sendMessageToReplica(
            *this, transport.config.primary_id(view_number), write);
    };
    auto send_to_all = [this, write] {
        transport.sendMessageToAll(*this, write);
    };
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

    transport.scheduleTimeout(
        config.resend_interval, [this, current_number = request_number] {
            if (!pending || request_number != current_number) {
                return;
            }
            warn("Resend: request number = {}", request_number);
            sendRequest(true);
        });
}

template <typename Transport, typename ReplicaMessage>
void BasicClient<Transport, ReplicaMessage>::handleReply(
    const ReplyMessage &reply)
{
    if (!pending) {
        return;
    }
    if (pending->request_number != reply.request_number) {
        return;
    }

    if (config.n_matched > 1) {
        pending->result_table.insert({reply.result, reply.replica_id});
        if (pending->result_table.count(reply.result) < config.n_matched) {
            return;
        }
    }

    auto callback = pending->callback;
    pending.reset();
    callback(reply.result);
}

} // namespace oscar