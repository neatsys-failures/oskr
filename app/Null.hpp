#pragma once
#include "core/Foundation.hpp"

namespace oscar
{
class NullApp : public App
{
public:
    Data commit(Data op) override
    {
        (void)op;
        return Data();
    }
    void rollback(Data op) override { (void)op; }
};
} // namespace oscar