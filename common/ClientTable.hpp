#pragma once
#include <optional>
#include <unordered_map>

#include "core/Foundation.hpp"

namespace oskr
{
template <typename Transport, typename ReplyMessage> class ClientTable
{
    struct Record {
        std::optional<typename Transport::Address> remote;
        RequestNumber request_number;
        std::optional<ReplyMessage> reply_message;
    };

    HashMap<ClientId, Record> record_table;

public:
    using Apply = Fn<void(
        FnOnce<void(
            const typename Transport::Address &remote, const ReplyMessage &)>)>;

    //! On handling direct request from client.
    //!
    //! If returned `Apply` is truthy, the request processing should be omit:
    //! ```
    //! if (auto apply =
    //!         client_table.check(remote, client_id, request_number)) {
    //!     apply([&](auto &remote, auto &reply) {
    //!         // send reply to remote
    //!     });
    //!     return;
    //! }
    //! // normally process new request here
    //! ```
    Apply check(
        const typename Transport::Address &remote, ClientId client_id,
        RequestNumber request_number);

    //! On handling relayed request message. Caller assumes `request_number`
    //! corresponding to an currently outstanding request.
    void update(ClientId client_id, RequestNumber request_number);

    //! On committing.
    //!
    //! Always return callable (instead of `nullptr`) even when nothing to do.
    Apply update(
        ClientId client_id, RequestNumber request_number, ReplyMessage reply);
};

template <typename Transport, typename ReplyMessage>
auto ClientTable<Transport, ReplyMessage>::check(
    const typename Transport::Address &remote, ClientId client_id,
    RequestNumber request_number) -> Apply
{
    auto iter = record_table.find(client_id);
    if (iter == record_table.end()) {
        record_table.insert(
            {client_id, {remote, request_number, std::nullopt}});
        return nullptr;
    }

    if (!iter->second.remote) {
        iter->second.remote = remote;
    }

    auto &record = iter->second;
    if (request_number < record.request_number) {
        return [](auto) {};
    }
    if (request_number == record.request_number) {
        if (!record.reply_message) {
            return [](auto) {};
        }
        return [remote, reply = *record.reply_message](auto on_reply) {
            on_reply(remote, reply);
        };
    }

    if (request_number != record.request_number + 1) {
        panic(
            "Not continuous request number: client id = {}, {} -> {}",
            client_id, record.request_number, request_number);
    }

    record.request_number = request_number;
    record.reply_message = std::nullopt;
    return nullptr;
}

template <typename Transport, typename ReplyMessage>
void ClientTable<Transport, ReplyMessage>::update(
    ClientId client_id, RequestNumber request_number)
{
    auto iter = record_table.find(client_id);
    if (iter == record_table.end()) {
        record_table.insert(
            {client_id, {std::nullopt, request_number, std::nullopt}});
        return;
    }

    if (iter->second.request_number >= request_number) {
        warn(
            "Ignore late update (request): client id = {:x}, request "
            "number = {}, recorded request = {}",
            client_id, request_number, iter->second.request_number);
        return;
    }

    iter->second.request_number = request_number;
    iter->second.reply_message.reset();
}

template <typename Transport, typename ReplyMessage>
auto ClientTable<Transport, ReplyMessage>::update(
    ClientId client_id, RequestNumber request_number, ReplyMessage reply)
    -> Apply
{
    auto iter = record_table.find(client_id);
    if (iter == record_table.end()) {
        warn("No record: client id = {:x}", client_id);
        record_table.insert({client_id, {std::nullopt, request_number, reply}});
        return [](auto) {};
    }

    if (iter->second.request_number > request_number) {
        warn(
            "Ignore late update: client id = {:x}, request number = {}, "
            "recorded request = {}",
            client_id, request_number, iter->second.request_number);
        return [](auto) {};
    }
    if (iter->second.request_number < request_number) {
        warn(
            "Outdated local record: client id = {:x}, request number = {}, "
            "recorded request = {}",
            client_id, request_number, iter->second.request_number);
        iter->second.request_number = request_number;
    }

    iter->second.reply_message = reply;
    if (!iter->second.remote) {
        debug("Client address not recorded: id = {:x}", client_id);
        return [](auto) {};
    }

    return [remote = *iter->second.remote, reply](auto on_reply) {
        on_reply(remote, reply);
    };
}
} // namespace oskr