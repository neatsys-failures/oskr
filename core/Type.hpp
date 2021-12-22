#pragma once
#include <cstdint>
#include <span>

#include <boost/container/small_vector.hpp>

namespace oscar
{
using OpNumber = std::uint64_t;
using RequestNumber = std::uint32_t;
using ViewNumber = std::uint32_t;

using ReplicaId = std::int8_t;
using ClientId = std::uint32_t;

// TODO configurable preallocate size
using Data = boost::container::small_vector<std::uint8_t, 16>;
// although value semantic is preferred, Span (slice) is critical in
// accomplishing zero-copy message processing
// Span for input
using Span = std::span<std::uint8_t>;
// Buffer for output
template <std::size_t BUFFER_SIZE> using Buffer = std::uint8_t[BUFFER_SIZE];
} // namespace oscar
