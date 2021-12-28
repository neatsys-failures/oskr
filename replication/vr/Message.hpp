#pragma once
#include "common/BasicClient.hpp"
#include "core/Foundation.hpp"

namespace oskr::vr
{
struct PrepareMessage {
    ViewNumber view_number;
    OpNumber op_number;
    Log<>::List::Block block;
    OpNumber commit_number;

    template <typename S> void serialize(S &s)
    {
        s(view_number, op_number, block, commit_number);
    }
};

struct PrepareOkMessage {
    ViewNumber view_number;
    OpNumber op_number;
    ReplicaId replica_id;

    template <typename S> void serialize(S &s)
    {
        s(view_number, op_number, replica_id);
    }
};

struct CommitMessage {
    ViewNumber view_number;
    OpNumber commit_number;

    template <typename S> void serialize(S &s)
    {
        s(view_number, commit_number);
    }
};

struct StartViewChangeMessage {
    ViewNumber view_number;
    ReplicaId replica_id;

    template <typename S> void serialize(S &s) { s(view_number, replica_id); }
};

struct DoViewChangeMessage {
    ViewNumber view_number;
    // TODO log
    ViewNumber latest_normal;
    OpNumber op_number, commit_number;

    template <typename S> void serialize(S &s)
    {
        s(view_number, latest_normal, op_number, commit_number);
    }
};

struct StartViewMessage {
    ViewNumber view_number;
    // TODO log
    OpNumber op_number, commit_number;

    template <typename S> void serialize(S &s)
    {
        s(view_number, op_number, commit_number);
    }
};

using ReplicaMessage = std::variant<
    RequestMessage, PrepareMessage, PrepareOkMessage, CommitMessage,
    StartViewChangeMessage, DoViewChangeMessage, StartViewMessage>;

} // namespace oskr::vr