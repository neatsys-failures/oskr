#pragma once
#include <functional>

#include "core/Type.hpp"
#include "core/Utility.hpp"

namespace oskr
{
class App
{
public:
    virtual ~App() {}
    virtual Data commit(Data op) = 0;
    virtual void rollback(Data) { panic("Unsupported rollback operation"); }
};

template <typename Preset = void> class Log
{
protected:
    // we don't have requirement to access `app` from outside after init
    // currently, but we still need polymorphism on it
    App &app;
    bool enable_upcall;

    Log(App &app) : app(app)
    {
        enable_upcall = true;
        //
    }

public:
    using Index = typename Preset::Index;
    using Block = typename Preset::Block;

    virtual ~Log() {}

    virtual void prepare(Index index, Block block) = 0;

    using ReplyCallback = Fn<void(ClientId, RequestNumber, Data)>;
    virtual void commit(Index index, ReplyCallback callback) = 0;
    // commitUpTo should be unusual for fast path

    virtual void rollbackTo(Index index) = 0;

    virtual void enableUpcall() = 0;

    virtual void disableUpcall() { enable_upcall = false; }
};

template <> struct Log<void> {
    using Entry = struct {
        ClientId client_id;
        RequestNumber request_number;
        Data op;
    };
    static constexpr int block_size = 50; // TODO
    // expect 600K~1M throughput @ <= 60 seconds
    static constexpr std::size_t n_reserved_entry = 80 * 1000 * 1000;

    struct ListPreset {
        using Index = OpNumber;
        using Block = struct {
            Entry entry_buffer[block_size];
            int n_entry;
        };
    };
    using List = Log<ListPreset>;

    struct ChainPreset {
        using Index = Hash;
        using Block = struct {
            Entry entry_buffer[block_size];
            int n_entry;
            Hash previous;
        };
    };
    using Chain = Log<ChainPreset>;
};

} // namespace oskr