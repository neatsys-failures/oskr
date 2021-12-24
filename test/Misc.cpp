#include <gtest/gtest.h>

#include "core/Foundation.hpp"
#include "transport/Simulated.hpp"

using namespace oscar; // NOLINT

class SimpleClient : public Client<SimulatedTransport>
{
public:
    explicit SimpleClient(SimulatedTransport &transport) :
        Client<SimulatedTransport>(transport)
    {
    }

    std::uint32_t GetId() const { return client_id; }

    void
    receiveMessage(const typename SimulatedTransport::Address &, Span) override
    {
    }

    void invoke(Data, InvokeCallback) override {}
};

TEST(Misc, ClientId)
{
    Config<SimulatedTransport> config{0, {}, {}};
    SimulatedTransport transport(config);
    SimpleClient client1(transport), client2(transport);
    ASSERT_NE(client1.GetId(), client2.GetId());
}

struct SimpleMessage {
    OpNumber op_number;
    Data data;

    template <typename S> void serialize(S &s) { s(op_number, data); }
};

TEST(Misc, Bitsery)
{
    SimpleMessage message{42, {12, 11}};
    auto write = [message](auto &buffer) {
        return bitserySerialize(buffer, message);
    };
    Buffer<100> buffer;
    std::size_t len = write(buffer);
    ASSERT_GT(len, 0);

    Span buffer_span(buffer, len);
    SimpleMessage out_message;
    bitseryDeserialize(buffer_span, out_message);
    ASSERT_EQ(out_message.op_number, 42);
    Data expected_data{12, 11};
    ASSERT_EQ(out_message.data, expected_data);
}
