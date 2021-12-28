//! @file

#pragma once
#include <cstdint>
#include <span>

#include <boost/container/small_vector.hpp>

namespace oskr
{
using OpNumber = std::uint64_t;
using RequestNumber = std::uint32_t;
using ViewNumber = std::uint32_t;

using ReplicaId = std::int8_t;
using ClientId = std::uint32_t;

using RxSpan = std::span<std::uint8_t>;
template <std::size_t BUFFER_SIZE>
using TxSpan = std::span<std::uint8_t, BUFFER_SIZE>;
//! For general storage of opaque things
//!
// TODO configurable preallocate size
using Data = boost::container::small_vector<std::uint8_t, 16>;

using Hash = std::uint8_t[32]; // SHA256
} // namespace oskr
