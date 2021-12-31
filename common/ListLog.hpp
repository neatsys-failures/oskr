#pragma once
#include <vector>

#include "core/Foundation.hpp"

namespace oskr
{
class ListLog : public Log<>::List
{
    struct FlattenBlock {
        std::size_t offset;
        int n_entry;
        bool committed;
    };
    std::vector<FlattenBlock> block_list;
    std::vector<Log<>::Entry> entry_list;
    OpNumber start_number, commit_number;

public:
    explicit ListLog(App &app) : Log<Log<>::ListPreset>(app)
    {
        start_number = 0;
        commit_number = 0;

#ifdef OSKR_BENCHMARK
        // guess what batch size will be used?
        // benchmark env should feel well even preallocate for no batch :)
        block_list.reserve(Log<>::n_reserved_entry);
        entry_list.reserve(Log<>::n_reserved_entry);
#endif
    }

    void prepare(OpNumber index, Block block) override
    {
        if (start_number == 0) {
            if (index != 1) {
                info("log start from the middle: start number = {}", index);
            }
            start_number = index;
            commit_number = start_number - 1;
        }

        if (blockOffset(index) != block_list.size()) {
            panic(
                "unexpected prepare: index = {}, expected = {}", index,
                start_number + block_list.size());
        }

        block_list.push_back({entry_list.size(), block.n_entry, false});
        entry_list.insert(
            entry_list.end(), block.entry_buffer,
            block.entry_buffer + block.n_entry);
    }

    void commit(OpNumber index, ReplyCallback callback) override
    {
        if (blockOffset(index) >= block_list.size()) {
            panic(
                "commit nonexist log entry: index = {}, latest = {}", index,
                start_number + block_list.size() - 1);
        }
        block_list[blockOffset(index)].committed = true;
        if (enable_upcall) {
            makeUpcall(callback);
        }
    }

    void rollbackTo(OpNumber index) override
    {
        if (start_number == 0) {
            return;
        }
        if (index < start_number) {
            block_list.clear();
            entry_list.clear();
            return;
        }
        std::size_t offset = block_list[blockOffset(index)].offset;
        block_list.erase(
            block_list.begin() + blockOffset(index), block_list.end());
        entry_list.erase(entry_list.begin() + offset, entry_list.end());
    }

    void enableUpcall() override
    {
        enable_upcall = true;
        makeUpcall([](auto, auto, auto) {});
    }

private:
    std::size_t blockOffset(OpNumber op_number)
    {
        if (start_number == 0) {
            panic("Cannot get block offset when start number not set");
        }
        return op_number - start_number;
    }

    void makeUpcall(ReplyCallback callback)
    {
        while (blockOffset(commit_number + 1) < block_list.size() &&
               block_list[blockOffset(commit_number + 1)].committed) {
            commit_number += 1;
            auto &block = block_list[blockOffset(commit_number)];
            for (int i = 0; i < block.n_entry; i += 1) {
                auto &entry = entry_list[block.offset + i];
                auto reply = app.commit(entry.op);
                callback(entry.client_id, entry.request_number, reply);
            }
        }
    }
};

} // namespace oskr