#pragma once
#include <optional>
#include <unordered_map>

#include "core/Foundation.hpp"

namespace oscar
{
template <typename Transport, typename ReplyMessage> class ClientTable
{
    struct Record {
        typename Transport::Address remote;
        RequestNumber request_number;
        std::optional<ReplyMessage> reply_message;
    };

    std::unordered_map<ClientId, Record> record_table;

public:
    using Apply = std::function<void(
        std::function<void(
            const typename Transport::Address &remote, const ReplyMessage &)>)>;
    Apply checkShortcut(
        const typename Transport::Address &remote, ClientId client_id,
        RequestNumber request_number)
    {
        auto iter = record_table.find(client_id);
        if (iter == record_table.end()) {
            record_table.insert(
                {client_id, {remote, request_number, std::nullopt}});
            return nullptr;
        }

        auto &record = iter->second;
        if (request_number < record.request_number) {
            return [](auto) {};
        }
        if (request_number == record.request_number) {
            if (!record.reply_message) {
                return [](auto) {};
            }
            return
                [&](auto on_reply) { on_reply(remote, *record.reply_message); };
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

    Apply update(ClientId client_id, ReplyMessage reply)
    {
        auto iter = record_table.find(client_id);
        if (iter == record_table.end()) {
            warn("No record: client id = {:x}", client_id);
            return [](auto) {};
        }
        iter->second.reply_message = reply;
        return [&](auto on_reply) { on_reply(iter->second.remote, reply); };
    }
};
} // namespace oscar