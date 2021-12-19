#include <gtest/gtest.h>

#include "core/Foundation.hpp"
#include "transport/Simulated.hpp"

using namespace oscar;

class SimpleClient : public Client<SimulatedTransport>
{
public:
    SimpleClient(SimulatedTransport &transport)
        : Client<SimulatedTransport>(transport)
    {
    }

    std::uint32_t GetId() const { return client_id; }

    void receiveMessage(
        const typename SimulatedTransport::Address &remote,
        const Span &buffer) override
    {
    }

    void Invoke(const Data op, InvokeCallback callback) override {}
};

TEST(CoreFoundation, ClientId)
{
    Config<SimulatedTransport> config{0, {}};
    SimulatedTransport transport(config);
    SimpleClient client1(transport), client2(transport);
    ASSERT_NE(client1.GetId(), client2.GetId());
}
