#pragma once
#include "core/Foundation.hpp"

namespace oskr
{
class NullApp : public App
{
public:
    Data commit(Data) override { return Data(); }
    void rollback(Data) override {}
};
} // namespace oskr