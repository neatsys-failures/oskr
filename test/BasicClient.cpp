#include <gtest/gtest.h>

#include "common/BasicClient.hpp"
#include "transport/Simulated.hpp"

using namespace oskr; // NOLINT

struct ClientTag;
template <> struct ClientSetting<ClientTag> {
    using ReplicaMessage = std::variant<oskr::RequestMessage>;
    static constexpr auto strategy = ClientSetting<>::Strategy::primary_first;
    static constexpr std::size_t fault_multiplier = 0;
    static constexpr std::chrono::microseconds resend_interval = 1000ms;
};
TEST(BasicClient, Noop)
{
    Config<Simulated> config{0, {}, {}};
    Simulated transport(config);
    BasicClient<Simulated, ClientTag> client(transport);
}
