#include <gtest/gtest.h>

#include "common/BasicClient.hpp"
#include "transport/Simulated.hpp"

using namespace oscar;
using namespace std::literals::chrono_literals;
using ReplicaMessage = std::variant<RequestMessage>;
using Strategy =
    BasicClient<SimulatedTransport, ReplicaMessage>::Config::Strategy;

template <typename S> void serialize(S &s, ReplicaMessage &message)
{
    s(message);
}

TEST(BasicClient, Noop)
{
    Config<SimulatedTransport> config{0, {}, {}};
    SimulatedTransport transport(config);
    BasicClient<SimulatedTransport, ReplicaMessage> client(
        transport, {Strategy::PRIMARY_FIRST, 1000ms, 1});
}
