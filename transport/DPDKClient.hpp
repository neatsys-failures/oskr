#pragma once
#include <vector>

#include <rte_eal.h>
#include <rte_ether.h>
#include <rte_mbuf.h>

#include "core/Foundation.hpp"

namespace oscar
{
class DPDKClient;
struct DPDKSpan {
    DPDKClient &transport;

    explicit DPDKSpan(struct rte_mbuf *, DPDKClient &transport) :
        transport(transport)
    {
    }
    ~DPDKSpan();

    DPDKSpan(const DPDKSpan &span) : transport(span.transport) {}
    DPDKSpan(const DPDKSpan &&span) : transport(span.transport) {}

    std::uint8_t *data() const { return nullptr; }
    std::size_t size() const { return 0; }
};

class DPDKClient;
template <> struct TransportMeta<DPDKClient> {
    using Address = std::pair<struct rte_ether_addr, std::uint16_t>;
    static constexpr std::size_t BUFFER_SIZE = 1500;
    using Span = DPDKSpan;
};

class DPDKClient : public TransportBase<DPDKClient>
{
public:
    const Config<DPDKClient> &config;

    DPDKClient(Config<DPDKClient> &config, char *prog_name) : config(config)
    {
        std::vector<char *> args{prog_name};
        if (rte_eal_init(args.size(), args.data()) < 0) {
            panic("EAL initialize failed");
        }
    }

    void registerReceiver(Address, Receiver) { panic("Todo"); }

    void spawn(Callback &&) { panic("Todo"); }

    template <typename Sender>
    void sendMessage(const Sender &, const Address &, Write)
    {
        panic("Todo");
    }

    void releaseSpan(DPDKSpan &) { panic("Todo"); }
};

DPDKSpan::~DPDKSpan()
{
    // TODO counting
    transport.releaseSpan(*this);
}

} // namespace oscar