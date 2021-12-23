#pragma once
#include <variant>

#include <bitsery/adapter/buffer.h>
#include <bitsery/bitsery.h>
#include <bitsery/brief_syntax.h>
#include <bitsery/brief_syntax/variant.h>
#include <bitsery/brief_syntax/vector.h>
#include <spdlog/spdlog.h>

#include "core/Client.hpp"
#include "core/Log.hpp"
#include "core/Transport.hpp"
#include "core/TransportReceiver.hpp"
#include "core/Type.hpp"

namespace oscar
{

using spdlog::debug, spdlog::info, spdlog::warn;

template <typename... Args>
[[noreturn]] void panic(spdlog::format_string_t<Args...> fmt, Args &&...args)
{
    spdlog::error(fmt, std::forward<Args>(args)...);
    std::abort();
}

// TODO explore a at-least-equally convinent but better way
#ifndef OSCAR_NO_RLOGGING
#define rdebug(fmt, ...) debug("[%d] " fmt, this->replica_id, ##__VA_ARGS__)
#define rinfo(fmt, ...) info("[%d] " fmt, this->replica_id, ##__VA_ARGS__)
#define rwarn(fmt, ...) warn("[%d] " fmt, this->replica_id, ##__VA_ARGS__)
#define rpanic(fmt, ...) panic("[%d] " fmt, this->replica_id, ##__VA_ARGS__)
#endif

template <typename Buffer, typename Message>
std::size_t bitserySerialize(Buffer &buffer, const Message &message)
{
    return bitsery::quickSerialization<
        bitsery::OutputBufferAdapter<Buffer>, Message>(buffer, message);
}

template <typename Message>
void bitseryDeserialize(const Span span, Message &message)
{
    auto state = bitsery::quickDeserialization<
        bitsery::InputBufferAdapter<std::uint8_t *>, Message>(
        {span.data(), span.size()}, message);
    if (state.first != bitsery::ReaderError::NoError || !state.second) {
        panic(
            "Deserialize failed: error = {}, completed = {}", state.first,
            state.second);
    }
}
} // namespace oscar

namespace bitsery
{
// should work... i think?
namespace traits
{
template <>
struct ContainerTraits<oscar::Data>
    : public StdContainer<oscar::Data, true, true> {
};
} // namespace traits

template <typename S> void serialize(S &s, oscar::Data &data)
{
    s.container1b(data, 240);
}

} // namespace bitsery