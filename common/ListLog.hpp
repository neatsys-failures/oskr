#pragma once
#include "core/Foundation.hpp"

namespace oscar
{
class ListLog : public Log<Log<>::ListPreset>
{
public:
    ListLog(App &app) : Log<Log<>::ListPreset>(app) {}

    void prepare(OpNumber index, Block block) override {}

    void commit(OpNumber index, ReplyCallback callback) override {}

    void rollbackTo(OpNumber index) override {}

    void enableUpcall() override {}
};

} // namespace oscar