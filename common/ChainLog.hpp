#pragma once
#include <unordered_map>

#include "core/Foundation.hpp"

namespace oscar
{
class ChainLog : public Log<>::Chain
{
    struct BlockBox {
        Block block;
        bool committed;
    };
    std::unordered_map<Hash, BlockBox> block_table;
    //
};
} // namespace oscar
