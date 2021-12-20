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
    using Shortcut =
        std::function<void(std::function<void(const ReplyMessage &)>)>;
    Shortcut checkShortcut(
        const typename Transport::Address &remote, ClientId client_id,
        RequestNumber request_number)
    {
        auto iter = record_table.find(client_id);
        if (iter == record_table.end()) {
            if (request_number != 1) { // TODO recover case?
                panic(
                    "Request number start from {}: client id = {}",
                    request_number, client_id);
            }
            record_table.insert(
                {client_id, {remote, request_number, std::nullopt}});
            return nullptr;
        }

        auto &record = iter->second;
        if (request_number < record.request_number) {
            return [](auto on_shortcut) { (void)on_shortcut; };
        }
        if (request_number == record.request_number) {
            if (!record.reply_message) {
                return [](auto on_shortcut) { (void)on_shortcut; };
            }
            return [&](auto on_shortcut) { on_shortcut(*record.reply_message); };
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
};
} // namespace oscar