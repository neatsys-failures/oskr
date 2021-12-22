#pragma once
#include <functional>

#include "core/Type.hpp"

namespace oscar
{
class App
{
public:
    virtual Data commit(Data op) = 0;
    virtual void rollback()
    {
        // panic unsupported
    }
};

template <typename Log> struct LogMeta {
    // using Index = ...;
    // using Block = ...;
};

template <typename Self> class Log
{
protected:
    App &app;
    bool enable_upcall;

    Log(App &app) : app(app)
    {
        enable_upcall = true;
        //
    }

public:
    struct Entry {
        ClientId client_id;
        RequestNumber request_number;
        Data op;
    };

    using Index = typename LogMeta<Self>::Index;
    using Block = typename LogMeta<Self>::Block;
    static constexpr std::size_t BLOCK_SIZE = 50; // TODO
    // expect 600K~1M throughput @ <= 60 seconds
    static constexpr std::size_t N_RESERVED_ENTRY = 80 * 1000 * 1000;

    virtual ~Log() {}

    virtual void prepare(Index index, Block block) = 0;

    using ReplyCallback = std::function<void(ClientId, RequestNumber, Data)>;
    virtual void commit(Index index, ReplyCallback callback) = 0;
    // commitUpTo should be unusual for fast path

    virtual void rollbackTo(Index index) = 0;

    virtual void enableUpcall() = 0;

    virtual void disableUpcall() { enable_upcall = false; }
};
} // namespace oscar