#pragma once
#include <vector>

#include "core/Type.hpp"

namespace oskr
{
template <typename Transport> struct Config {
    std::size_t n_fault;
    std::vector<typename Transport::Address> replica_address_list;
    typename Transport::Address multicast_address{};

    int primaryId(ViewNumber view_number) const
    {
        return view_number % replica_address_list.size();
    }
};

} // namespace oskr
