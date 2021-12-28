#include "app/Null.hpp"
#include "common/ListLog.hpp"
#include "replication/unreplicated/Replica.hpp"
#include "transport/DPDKClient.hpp"

using namespace oscar;

int main(int, char *argv[])
{
    Config<DPDKClient> config{
        0,
        {
            {{0x00, 0x15, 0x5d, 0xa0, 0x24, 0x09}, 0},
        }};
    DPDKClient transport(config, argv[0]);
    info("Transport initialized");

    NullApp app;
    ListLog log(app);

    unreplicated::Replica replica(transport, log);
    return 0;
}
