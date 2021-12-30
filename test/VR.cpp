#include <gtest/gtest.h>

#include "app/Mock.hpp"
#include "common/ListLog.hpp"
#include "replication/vr/Client.hpp"
#include "replication/vr/Replica.hpp"
#include "transport/Simulated.hpp"

using namespace oskr;     // NOLINT
using namespace oskr::vr; // NOLINT
using namespace std;      // NOLINT

class VR : public testing::Test
{
    Config<Simulated> config;
    Simulated transport;
    vector<MockApp> app;
    vector<ListLog> log;
    vector<Replica<Simulated>> replica;
    vector<vr::Client<Simulated>> client;

    VR() :
        config{1, {"replica-0", "replica-1", "replica-2", "replica-3"}, {}},
        transport(config)
    {
        for (int i = 0; i < config.n_replica(); i += 1) {
            app.push_back(MockApp{});
            log.push_back(ListLog(app.back()));
            replica.push_back(Replica(transport, log.back(), i, 1));
        }
    }

    void spawnClient(int n_client)
    {
        //
    }
};

TEST(VR, Noop) {}
