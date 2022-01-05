#include "replication/unreplicated/Client.hpp"
#include "app/Null.hpp"
#include "common/ListLog.hpp"
#include "transport/DPDKClient.hpp"

using namespace oskr; // NOLINT

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

    unreplicated::Client<DPDKClient> client(transport);
    return 0;
}
