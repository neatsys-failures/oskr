#include <gtest/gtest.h>

#include "common/BasicClient.hpp"
#include "transport/Simulated.hpp"

using namespace oscar; // NOLINT
using ReplicaMessage = std::variant<RequestMessage>;

TEST(BasicClient, Noop)
{
    Config<SimulatedTransport> config{0, {}, {}};
    SimulatedTransport transport(config);
    BasicClient<SimulatedTransport, ReplicaMessage> client(
        transport, {BasicClient<>::Config::Strategy::PRIMARY_FIRST, 1000ms, 1});
}
