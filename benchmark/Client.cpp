#include <rte_cycles.h>
#include <rte_eal.h>

#include "transport/DPDKClient.hpp"

using oscar::info;

int main(int argc, char *argv[])
{
    rte_eal_init(argc, argv);
    info("Client start: TSC = {}hz", rte_get_tsc_hz());
    uint64_t instant = rte_rdtsc();
    rte_delay_us_sleep(64 * 1000);
    info(
        "Sleep end after {} sec",
        static_cast<long double>(rte_rdtsc() - instant) / rte_get_tsc_hz());
    return 0;
}