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
    static constexpr auto strategy = ClientSetting<>::Strategy::primary_first;
    static constexpr std::size_t fault_multiplier = 0;
    static constexpr std::chrono::microseconds resend_interval = 1000ms;
};
} // namespace oskr
namespace oskr::vr
{
template <TransportTrait Transport>
using Client = BasicClient<Transport, ClientTag>;
}