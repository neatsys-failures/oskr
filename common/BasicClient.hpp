#pragma once
#include "core/Foundation.hpp"

namespace oscar
{
template <typename Transport> class BasicClient : public Client<Transport>
{
    RequestNumber request_number;
    bool pending;
    Data op;
    typename Client<Transport>::InvokeCallback callback;

public:
    struct Config {
        std::size_t n_matched;
        enum Strategy {
            UNSPECIFIED,
            ALL,
            PRIMARY_FIRST,
        } strategy;
    } config;

    BasicClient(Transport &transport, Config config)
        : Client<Transport>(transport)
    {
        this->config = config;

        request_number = 0;
        pending = false;
    }

    void invoke(
        Data op, typename Client<Transport>::InvokeCallback callback) override
    {
        // TODO
    }

    void receiveMessage(
        const typename Transport::Address &remote, const Span &span) override
    {
        //
    }
};
} // namespace oscar