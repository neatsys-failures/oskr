//! @file

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

// for rx
//! Unlike `Buffer`, `Span` does not own represented slice, so using `Span` with
//! value semantic actually behaves as pass reference.
using Span = const std::span<std::uint8_t>;
// for tx
//! The thing similar to `Span` is `Buffer &`, and use `auto &` in lambdas.
//!
template <std::size_t BUFFER_SIZE> using Buffer = std::uint8_t[BUFFER_SIZE];
//! For general storage of opaque things
//!
// TODO configurable preallocate size
using Data = boost::container::small_vector<std::uint8_t, 16>;

using Hash = std::uint8_t[32]; // SHA256
} // namespace oscar
