#pragma once
#include "common/BasicClient.hpp"
#include "replication/vr/Message.hpp"

namespace oskr::vr
{
struct ClientTag {
};
} // namespace oskr::vr
namespace oskr
{
template <> struct ClientSetting<vr::ClientTag> {
    using ReplicaMessage = vr::ReplicaMessage;
    static constexpr auto STRATEGY = ClientSetting<>::Strategy::PRIMARY_FIRST;
    static constexpr std::size_t N_MATCHED = 1;
    static constexpr std::chrono::microseconds RESEND_INTERVAL = 1000ms;
};
} // namespace oskr
namespace oskr::vr
{
template <TransportTrait Transport>
using Client = BasicClient<Transport, ClientTag>;
}