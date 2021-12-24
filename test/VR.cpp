#include <gtest/gtest.h>

#include "app/Mock.hpp"
#include "common/ListLog.hpp"
#include "replication/vr/Replica.hpp"
#include "transport/Simulated.hpp"

using namespace oscar;     // NOLINT
using namespace oscar::vr; // NOLINT

TEST(VR, Noop)
{
    Config<SimulatedTransport> config{0, {"replica-0"}, {}};
    SimulatedTransport transport(config);
    MockApp app;
    ListLog log(app);
    Replica replica(transport, log, 0, 1);
}