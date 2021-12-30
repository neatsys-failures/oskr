#pragma once
#include <random>

#include <bitsery/adapter/buffer.h>
#include <bitsery/bitsery.h>
#include <bitsery/brief_syntax.h>
#include <bitsery/brief_syntax/vector.h>
#include <folly/logging/Init.h>
#include <folly/logging/xlog.h>
// #include <spdlog/spdlog.h>

#include "core/Type.hpp"

namespace oskr
{
using //
    std::literals::chrono_literals::operator""ms,
    std::literals::chrono_literals::operator""us;

#define debug(fmt, ...) XLOG(DBG0, fmt, ##__VA_ARGS__)
#define info(fmt, ...) XLOG(INFO, fmt, ##__VA_ARGS__)
#define warn(fmt, ...) XLOG(WARN, fmt, ##__VA_ARGS__)
#define panic(fmt, ...) XLOG(FATAL, fmt, ##__VA_ARGS__)

// TODO explore a at-least-equally convinent but better way
#ifndef OSKR_NO_RLOGGING
#define rdebug(fmt, ...) debug("[%d] " fmt, this->replica_id, ##__VA_ARGS__)
#define rinfo(fmt, ...) info("[%d] " fmt, this->replica_id, ##__VA_ARGS__)
#define rwarn(fmt, ...) warn("[%d] " fmt, this->replica_id, ##__VA_ARGS__)
#define rpanic(fmt, ...) panic("[%d] " fmt, this->replica_id, ##__VA_ARGS__)
#endif

std::default_random_engine &random_engine()
{
    static thread_local std::random_device seed;
    static thread_local std::default_random_engine engine(seed());
    return engine;
}

template <typename Message, std::size_t BUFFER_SIZE>
std::size_t bitserySerialize(TxSpan<BUFFER_SIZE> buffer, const Message &message)
{
    return bitsery::quickSerialization<
        bitsery::OutputBufferAdapter<std::uint8_t[BUFFER_SIZE]>, Message>(
        *(std::uint8_t(*)[BUFFER_SIZE])buffer.data(), message);
}

template <typename Message>
void bitseryDeserialize(const RxSpan span, Message &message)
{
    auto state = bitsery::quickDeserialization<
        bitsery::InputBufferAdapter<std::uint8_t *>, Message>(
        {span.data(), span.size()}, message);
    // TODO consider protect again malicious message under BFT situation
    if (state.first != bitsery::ReaderError::NoError || !state.second) {
        panic(
            "Deserialize failed: error = {}, completed = {}", state.first,
            state.second);
    }
}
} // namespace oskr
