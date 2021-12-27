#pragma once
#include <vector>

#include <rte_eal.h>

#include "core/Foundation.hpp"

namespace oscar
{
class DPDKClient;
template <> struct TransportMeta<DPDKClient> {
    using Address = std::pair<std::uint8_t[6], std::uint16_t>;
    static constexpr std::size_t BUFFER_SIZE = 1500;
};

class DPDKClient : public Transport<DPDKClient>
{
public:
    DPDKClient(Config<DPDKClient> &config, char *prog_name) :
        Transport<DPDKClient>(config)
    {
        std::vector<char *> args{prog_name};
        // TODO conditional
        // args.push_back("--no-huge");
        if (rte_eal_init(args.size(), args.data()) < 0) {
            panic("EAL initialize failed");
        }
    }

    Address allocateAddress() override { return Address(); }

    void registerReceiver(TransportReceiver<DPDKClient> &receiver) override {}
    void registerMulticastReceiver(
        TransportMulticastReceiver<DPDKClient> &receiver) override
    {
    }

    void
    spawn(std::chrono::microseconds delay, Callback callback) override
    {
    }

    void spawn(Callback callback) override
    {
        spawn(std::chrono::microseconds(0), callback);
    }
    void spawnConcurrent(Callback callback) {}

    int getConcurrentId() const override {}
    void sendMessage(
        const TransportReceiver<DPDKClient> &sender, const Address &dest,
        Write write) override
    {
    }
};
} // namespace oscar