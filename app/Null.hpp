#pragma once
#include "core/Foundation.hpp"

namespace oscar
{
class NullApp : public App
{
public:
    Data commit(Data) override { return Data(); }
    void rollback(Data) override {}
};
} // namespace oscar