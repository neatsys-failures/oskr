// have to extract bitsery-required specilization out to prevent cyclic
// including
#pragma once
#include <bitsery/traits/core/std_defaults.h>

#include "core/Log.hpp"

namespace bitsery::traits
{
// should work... i think?
template <>
struct ContainerTraits<oskr::Data>
    : public StdContainer<oskr::Data, true, true> {
};
} // namespace bitsery::traits

namespace bitsery // interesting fact: oskr::Data not belong to oskr namespace
{
template <typename S> void serialize(S &s, oskr::Data &data)
{
    s.container1b(data, 240);
}

template <typename S> void serialize(S &s, oskr::Log<>::Entry &entry)
{
    s(entry.client_id, entry.request_number, entry.op);
}

template <typename S> void serialize(S &s, oskr::Log<>::List::Block &block)
{
    s(block.n_entry);
    for (int i = 0; i < block.n_entry; i += 1) {
        s(block.entry_buffer[i]);
    }
}
} // namespace bitsery
