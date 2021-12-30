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
    static constexpr auto STRATEGY = ClientSetting<>::Strategy::PRIMARY_FIRST;
    static constexpr std::size_t N_MATCHED = 1;
    static constexpr std::chrono::microseconds RESEND_INTERVAL = 1000ms;
};
} // namespace oskr
namespace oskr::unreplicated
{
template <TransportTrait Transport>
using Client = BasicClient<Transport, ClientTag>;
}