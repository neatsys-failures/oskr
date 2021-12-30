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

// we only support view change under no-drop network, when no log transfer
// needed
// if we don't want protocol to panic too much, new primary may give up its view
// when missing packets, and new backups may do state transfer
struct ZeroLog {
    template <typename S> void serialize(S &s) {}
};

struct DoViewChangeMessage {
    ViewNumber view_number;
    ZeroLog log;
    ViewNumber latest_normal;
    OpNumber op_number, commit_number;

    template <typename S> void serialize(S &s)
    {
        s(view_number, log, latest_normal, op_number, commit_number);
    }
};

struct StartViewMessage {
    ViewNumber view_number;
    ZeroLog log;
    OpNumber op_number, commit_number;

    template <typename S> void serialize(S &s)
    {
        s(view_number, log, op_number, commit_number);
    }
};

using ReplicaMessage = std::variant<
    RequestMessage, PrepareMessage, PrepareOkMessage, CommitMessage,
    StartViewChangeMessage, DoViewChangeMessage, StartViewMessage>;

} // namespace oskr::vr