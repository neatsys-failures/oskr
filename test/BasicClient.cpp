#include <gtest/gtest.h>

#include "common/BasicClient.hpp"
#include "transport/Simulated.hpp"

using namespace oskr; // NOLINT

struct ClientTag;
template <> struct ClientSetting<ClientTag> {
    using ReplicaMessage = std::variant<oskr::RequestMessage>;
    static constexpr auto STRATEGY = ClientSetting<>::Strategy::PRIMARY_FIRST;
    static constexpr std::size_t N_MATCHED = 1;
    static constexpr std::chrono::microseconds RESEND_INTERVAL = 1000ms;
};
TEST(BasicClient, Noop)
{
    Config<Simulated> config{0, {}, {}};
    Simulated transport(config);
    BasicClient<Simulated, ClientTag> client(transport);
}
