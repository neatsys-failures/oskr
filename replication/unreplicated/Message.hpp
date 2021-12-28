#pragma once
#include "common/BasicClient.hpp"
#include "core/Foundation.hpp"

namespace oskr::unreplicated
{
using ReplicaMessage = std::variant<RequestMessage>;
}