#pragma once
#include <bitsery/adapter/buffer.h>
#include <bitsery/bitsery.h>
#include <bitsery/brief_syntax.h>
#include <bitsery/brief_syntax/vector.h>
#include <spdlog/spdlog.h>

#include "core/Client.hpp"
#include "core/TransportReceiver.hpp"
#include "core/Type.hpp"

namespace oscar
{
template <typename Buffer, typename Message>
std::size_t serialize(Buffer &buffer, const Message &message)
{
    return bitsery::quickSerialization<
        bitsery::OutputBufferAdapter<Buffer>, Message>(buffer, message);
}
template <typename Message> void deserialize(const Span &span, Message &message)
{
    auto state = bitsery::quickDeserialization<
        bitsery::InputBufferAdapter<std::uint8_t *>, Message>(
        {span.data(), span.size()}, message);
    if (state.first != bitsery::ReaderError::NoError || !state.second) {
        // TODO crash
    }
}

// TODO log macro

} // namespace oscar