#include "transport/DPDKClient.hpp"

using oscar::DPDKClient, oscar::Config;
using oscar::info;

int main(int argc, char *argv[])
{
    Config<DPDKClient> config{0, {}};
    DPDKClient transport(config, argv[0]);
    info("Transport initialized");
    return 0;
}
