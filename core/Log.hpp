#pragma once
#include <vector>

#include "core/Type.hpp"

namespace oscar
{
class Log
{
    struct Entry {
        OpNumber op_number;
        ClientId client_id;
        RequestNumber request_number;
        Data op;
    };
    std::vector<Entry> entry_list;

public:
    virtual ~Log() {}

    virtual void commitUpcall(const Entry &entry) {}
};
} // namespace oscar