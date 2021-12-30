#pragma once
#include "common/BasicClient.hpp"
#include "replication/unreplicated/Message.hpp"

namespace oskr::unreplicated
{
struct ClientTag {
};
} // namespace oskr::unreplicated
namespace oskr
{
template <> struct ClientSetting<unreplicated::ClientTag> {
    using ReplicaMessage = unreplicated::ReplicaMessage;
    static constexpr auto strategy = ClientSetting<>::Strategy::primary_first;
    static constexpr std::size_t fault_multiplier = 0;
    static constexpr std::chrono::microseconds resend_interval = 1000ms;
};
} // namespace oskr
namespace oskr::unreplicated
{
template <TransportTrait Transport>
using Client = BasicClient<Transport, ClientTag>;
}