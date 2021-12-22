#pragma once
#include <vector>

#include "core/Foundation.hpp"

namespace oscar
{
class MockApp : public App
{
    std::function<Data(Data)> make_reply;

public:
    std::vector<Data> op_list;

    MockApp(std::function<Data(Data)> make_reply = nullptr)
    {
        if (!make_reply) {
            make_reply = [](Data op) {
                std::string reply_string =
                    "Re: " + std::string(op.begin(), op.end());
                return Data(reply_string.begin(), reply_string.end());
            };
        }
        this->make_reply = make_reply;
    }

    Data commit(Data op) override
    {
        op_list.push_back(op);
        return make_reply(op);
    }
};
} // namespace oscar