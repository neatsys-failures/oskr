#pragma once
#include "core/Foundation.hpp"

namespace oscar
{
class NullApp : public App
{
public:
    Data commit(Data op) override {}
    void rollback() override {}
};
} // namespace oscar