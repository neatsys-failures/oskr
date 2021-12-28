#include <gtest/gtest.h>

#include "core/Foundation.hpp"
#include "transport/Simulated.hpp"

using namespace oskr; // NOLINT

class SimpleClient : public Client<Simulated>
{
public:
    explicit SimpleClient(Simulated &transport) : Client<Simulated>(transport)
    {
    }

    std::uint32_t GetId() const { return client_id; }

    void receiveMessage(const typename Simulated::Address &, RxSpan) override {}

    void invoke(Data, InvokeCallback) override {}
};

TEST(Misc, ClientId)
{
    Config<Simulated> config{0, {}, {}};
    Simulated transport(config);
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
    auto write = [message](auto buffer) {
        return bitserySerialize(buffer, message);
    };
    std::uint8_t buffer[100];
    std::size_t len = write(TxSpan<100>(buffer));
    ASSERT_GT(len, 0);

    std::span buffer_span(buffer, len);
    SimpleMessage out_message;
    bitseryDeserialize(buffer_span, out_message);
    ASSERT_EQ(out_message.op_number, 42);
    Data expected_data{12, 11};
    ASSERT_EQ(out_message.data, expected_data);
}
