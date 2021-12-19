#include <gtest/gtest.h>

#include "core/Foundation.hpp"
#include "transport/Simulated.hpp"

using namespace oscar;
using bitsery::Deserializer;
using bitsery::InputBufferAdapter;
using bitsery::OutputBufferAdapter;
using bitsery::Serializer;

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

struct SimpleMessage {
    OpNumber op_number;
    Data data;

    template <typename S> void serialize(S &s) { s(op_number, data); }
};

TEST(CoreFoundation, Bitsery)
{
    Config<SimulatedTransport> config{0, {}};
    SimulatedTransport transport(config);

    SimpleMessage message{42, {12, 11}};
    constexpr std::size_t N = SimulatedTransport::BUFFER_SIZE;
    auto write = [message](typename SimulatedTransport::Buffer &buffer) {
        return serialize(buffer, message);
    };
    typename SimulatedTransport::Buffer buffer;
    std::size_t len = write(buffer);
    ASSERT_GT(len, 0);

    Span buffer_span(buffer, len);
    SimpleMessage out_message;
    deserialize(buffer_span, out_message);
    ASSERT_EQ(out_message.op_number, 42);
    Data expected_data{12, 11};
    ASSERT_EQ(out_message.data, expected_data);
}
