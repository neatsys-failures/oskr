#pragma once
#include <vector>

#include "core/Foundation.hpp"

namespace oscar
{
class ListLog : public Log<>::List
{
    struct BlockBox {
        Block block;
        bool committed;
    };
    std::vector<BlockBox> block_list;
    OpNumber start_number, commit_number;

public:
    explicit ListLog(App &app) : Log<Log<>::ListPreset>(app)
    {
        start_number = 0;
        commit_number = 0;

#ifdef OSCAR_BENCHMARK
        block_list.reserve(Log<>::N_RESERVED_ENTRY / Log<>::BLOCK_SIZE);
#endif
    }

    void prepare(OpNumber index, Block block) override
    {
        if (start_number == 0) {
            start_number = index;
            commit_number = start_number - 1;
        }

        if (getIndex(index) != block_list.size()) {
            panic(
                "Unexpected prepare: index = {}, expected = {}", index,
                start_number + block_list.size());
        }

        block_list.push_back({block, false});
    }

    void commit(OpNumber index, ReplyCallback callback) override
    {
        block_list[getIndex(index)].committed = true;
        if (!enable_upcall) {
            return;
        }
        makeUpcall(callback);
    }

    void rollbackTo(OpNumber index) override
    {
        if (start_number == 0) {
            return;
        }
        if (index < start_number) {
            block_list.clear();
            return;
        }
        block_list.erase(
            block_list.begin() + getIndex(index), block_list.end());
    }

    void enableUpcall() override
    {
        enable_upcall = true;
        makeUpcall([](auto, auto, auto) {});
    }

private:
    std::size_t getIndex(OpNumber op_number)
    {
        if (start_number == 0) {
            panic("Cannot get index when start_number not set");
        }
        return op_number - start_number;
    }

    void makeUpcall(ReplyCallback callback)
    {
        while (getIndex(commit_number + 1) < block_list.size() &&
               block_list[getIndex(commit_number + 1)].committed) {
            commit_number += 1;
            auto &block = block_list[getIndex(commit_number)].block;
            for (int i = 0; i < block.n_entry; i += 1) {
                auto &entry = block.entry_buffer[i];
                auto reply = app.commit(entry.op);
                callback(entry.client_id, entry.request_number, reply);
            }
        }
    }
};

} // namespace oscar