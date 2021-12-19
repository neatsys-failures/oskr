#pragma once
#include <cstdint>
#include <vector>

#include <boost/core/span.hpp>

namespace oscar
{
using OpNumber = std::uint64_t;

using Data = std::vector<std::uint8_t>;
// although value semantic is preferred, Span (slice) is critical in
// accomplishing zero-copy message processing
using Span = boost::span<std::uint8_t>;
} // namespace oscar