#include <gtest/gtest.h>

#include "common/BasicClient.hpp"
#include "transport/Simulated.hpp"

using namespace oscar;

TEST(BasicClient, Noop)
{
    Config<SimulatedTransport> config{0, {}, {}};
    SimulatedTransport transport(config);
    BasicClient client(
        transport,
        {1, BasicClient<SimulatedTransport>::Config::Strategy::PRIMARY_FIRST});
}
