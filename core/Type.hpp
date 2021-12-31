//! @file

#pragma once
#include <cstdint>
#include <span>

#include <folly/Function.h>
#include <folly/container/F14Map.h>
#include <folly/small_vector.h>

namespace oskr
{
//! The ID type for consensus.
//!
//! @note Because batching is built into `Log` abstraction by now, normally one
//! `OpNumber` corresponds to a block/batch instead of one op.
using OpNumber = std::uint64_t;
using RequestNumber = std::uint32_t;
// We are doing researching here, there should not be more than hundreds of
// primary fault for one run under any case :)
//! @note While `OpNumber` and `RequestNumber` normally start from 1, and leave
//! 0 as invalid value, `ViewNumber` normally start from 0 so first primary id
//! would be 0.
using ViewNumber = std::uint8_t;

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

template <typename T> using Fn = std::function<T>;
template <typename T> using FnOnce = folly::Function<T>;
template <typename K, typename V> using HashMap = folly::F14FastMap<K, V>;
} // namespace oskr
