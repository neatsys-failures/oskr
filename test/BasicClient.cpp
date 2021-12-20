#include <gtest/gtest.h>

#include "common/BasicClient.hpp"
#include "transport/Simulated.hpp"

using namespace oscar;
using namespace std::literals::chrono_literals;
using Strategy = BasicClient<SimulatedTransport>::Config::Strategy;

TEST(BasicClient, Noop)
{
    Config<SimulatedTransport> config{0, {}, {}};
    SimulatedTransport transport(config);
    BasicClient client(transport, {Strategy::PRIMARY_FIRST, 1000ms, 1});
}
