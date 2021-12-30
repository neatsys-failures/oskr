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
        all,
        primary_first,
    };
};

/*! @brief General client logic that can be reused across several protocols,
designed for replication protocols.

The client does the following (and requires):
- When invoked, construct a `RequestMessage` and send to replica. The message
will be converted to replica message type before serializing, so replica message
should be able to be converted from `RequestMessage`, e.g.,
`std::variant<RequestMessage, ...>`.
- The message will be sent according to configured strategy:
  - `all` send to all replicas on everything (re)sending
  - `primary_first` send to primary according to learned view first, then send
    to every replicas on everything resending
- Send one request at a time. Cannot be `invoke`d again before callback
triggered.
- Assume replica will reply `ReplyMessage`.
- Finalize an invoking after collecting enough number of matched replies. The
number is configured as `fault_multiplier`, which means should collect
`fault_multiplier * n_fault + 1` replies. For example, for unreplicated and VR
`fault_multiplier` should be 0, means only receives one reply, and
`fault_multiplier` should be 1 for PBFT, etc.

* @note The replies must be matched. Crash-tolerence protocols may not
require/just assume matching but anyway.

If need advanced customization, `BasicClient` can be inherited and subclass can
override `serializeRequestMessage` and `deserializeReplyMessage`. For example
BFT protocols may inject message signing/verifying inside.
*/
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

protected:
    //! Notice that this method accept `ReplicaMessage`, so implementation do
    //! not convert from `RequestMessage` inside.
    virtual std::size_t serializeRequestMessage(
        TxSpan<Transport::buffer_size> buffer,
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

    switch (ClientSetting<Protocol>::strategy) {
    case ClientSetting<>::Strategy::all:
        send_to_all();
        break;
    case ClientSetting<>::Strategy::primary_first:
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
        ClientSetting<Protocol>::resend_interval,
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

    std::size_t n_matched =
        ClientSetting<Protocol>::fault_multiplier * transport.config.n_fault +
        1;
    if (n_matched > 1) {
        pending->result_table[reply.result].insert(reply.replica_id);
        if (pending->result_table.at(reply.result).size() < n_matched) {
            return;
        }
    }

    auto callback = std::move(pending->callback);
    pending.reset();
    callback(reply.result);
}

} // namespace oskr