#include <gtest/gtest.h>

#include "common/BasicClient.hpp"
#include "transport/Simulated.hpp"

using namespace oscar; // NOLINT
using ReplicaMessage = std::variant<RequestMessage>;

TEST(BasicClient, Noop)
{
    Config<Simulated> config{0, {}, {}};
    Simulated transport(config);
    BasicClient<Simulated, ReplicaMessage> client(
        transport, {BasicClient<>::Config::Strategy::PRIMARY_FIRST, 1000ms, 1});
}
