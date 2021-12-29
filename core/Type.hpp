//! @file

#pragma once
#include <cstdint>
#include <span>

// #include <boost/container/small_vector.hpp>
#include <folly/small_vector.h>

namespace oskr
{
using OpNumber = std::uint64_t;
using RequestNumber = std::uint32_t;
using ViewNumber = std::uint32_t;

using ReplicaId = std::int8_t;
using ClientId = std::uint32_t;

//! A view type for RX messages. The length is specified at runtime.
//!
//! Actually this type does not show in the signature of model abstraction.
//! Receiver closure accepts a `Desc` type of certain transport implementation,
//! which manages the lifetime of message-related resource. A `RxSpan` can be
//! easily construct with it:
//! ```
//! RxSpan span(desc.data(), desc.size());
//! ```
using RxSpan = std::span<std::uint8_t>;
//! A view type for (will become) TX messages. The available length is specified
//! at compile time. Transport implementation does not need to provide dynamical
//! message buffer.
template <std::size_t BUFFER_SIZE>
using TxSpan = std::span<std::uint8_t, BUFFER_SIZE>;
//! For general storage of opaque things.
//!
// TODO configurable preallocate size
// using Data = boost::container::small_vector<std::uint8_t, 16>;
using Data = folly::small_vector<std::uint8_t, 16>;

using Hash = std::uint8_t[32]; // SHA256
} // namespace oskr
