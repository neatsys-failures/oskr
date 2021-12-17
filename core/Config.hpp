#pragma once
#include <vector>

namespace oscar
{
template <typename Transport> struct Config {
    std::size_t n_fault;
    std::vector<typename Transport::Address> replica_address_list;
    typename Transport::Address multicast_address;
};

} // namespace oscar
