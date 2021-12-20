#pragma once
#include "common/BasicClient.hpp"
#include "core/Foundation.hpp"

namespace oscar::unreplicated
{
using ReplicaMessage = std::variant<RequestMessage>;
}