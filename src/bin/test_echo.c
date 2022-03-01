#include <rte_mbuf.h>
#include <rte_eal.h>
#include <rte_ethdev.h>
#include <rte_ether.h>
#include <rte_atomic.h>
#include <assert.h>
#include <pthread.h>
#include <unistd.h>

struct address_t
{
    struct rte_ether_addr mac;
    uint8_t id;
};

void *monitor(void *arg)
{
    rte_atomic32_t *count = arg;
    for (;;)
    {
        sleep(1);
        uint32_t c = rte_atomic32_exchange(&count->cnt, 0);
        printf("%u\n", c);
    }
    return NULL;
}

int main(int argc, char *argv[])
{
    char *eal_args[] = {"app/test_echo", "-c", "0x01"};
    int eal_argc = sizeof(eal_args) / sizeof(char *);

    int ret;

    ret = rte_eal_init(eal_argc, eal_args);
    assert(ret == eal_argc - 1);

    uint16_t port_id = 0;
    int n_concurrent = 1;

    struct address_t server_address;
    ret = rte_ether_unformat_addr("b8:ce:f6:2a:2f:94", &server_address.mac);
    assert(ret == 0);
    server_address.id = 0;

    bool invoke = argc > 1 && strcmp(argv[1], "invoke") == 0;
    struct address_t address;
    if (invoke)
    {
        rte_eth_macaddr_get(port_id, &address.mac);
        address.id = 254;
        if (argc > 2)
        {
            n_concurrent = atoi(argv[2]);
        }
    }
    else
    {
        rte_ether_addr_copy(&server_address.mac, &address.mac);
        address.id = server_address.id;
    }
    printf(RTE_ETHER_ADDR_PRT_FMT "#%u\n", RTE_ETHER_ADDR_BYTES(&address.mac), address.id);

    struct rte_mempool *mbuf_pool = rte_pktmbuf_pool_create(
        "MBUF_POOL", 8191, 250, 0, RTE_MBUF_DEFAULT_BUF_SIZE, rte_eth_dev_socket_id(port_id));
    assert(mbuf_pool != NULL);

    struct rte_eth_conf port_conf;
    memset(&port_conf, 0, sizeof(port_conf));
    struct rte_eth_dev_info dev_info;
    ret = rte_eth_dev_info_get(port_id, &dev_info);
    assert(ret == 0);
    if (dev_info.tx_offload_capa & RTE_ETH_TX_OFFLOAD_MBUF_FAST_FREE)
    {
        port_conf.txmode.offloads |= RTE_ETH_TX_OFFLOAD_MBUF_FAST_FREE;
    }

    ret = rte_eth_dev_configure(port_id, 1, 1, &port_conf);
    assert(ret == 0);
    uint16_t n_rx_desc = 1024, n_tx_desc = 1024;
    ret = rte_eth_dev_adjust_nb_rx_tx_desc(port_id, &n_rx_desc, &n_tx_desc);
    assert(ret == 0);
    ret = rte_eth_rx_queue_setup(port_id, 0, n_rx_desc, rte_eth_dev_socket_id(port_id), NULL, mbuf_pool);
    assert(ret == 0);
    struct rte_eth_txconf txconf = dev_info.default_txconf;
    txconf.offloads = port_conf.txmode.offloads;
    ret = rte_eth_tx_queue_setup(port_id, 0, n_tx_desc, rte_eth_dev_socket_id(port_id), &txconf);
    assert(ret == 0);

    ret = rte_eth_dev_start(port_id);
    assert(ret == 0);
    ret = rte_eth_promiscuous_enable(port_id);
    assert(ret == 0);

    rte_atomic32_t count;
    rte_atomic32_init(&count);
    rte_atomic32_set(&count, 0);
    pthread_t monitor_thread;
    ret = pthread_create(&monitor_thread, NULL, monitor, &count);
    assert(ret == 0);

    if (invoke)
    {
        for (int i = 0; i < n_concurrent; i += 1)
        {
            struct rte_mbuf *buf = rte_pktmbuf_alloc(mbuf_pool);
            buf->pkt_len = buf->data_len = 16;
            struct rte_ether_hdr *ether_hdr = rte_pktmbuf_mtod(buf, struct rte_ether_hdr *);
            uint8_t *ids = rte_pktmbuf_mtod_offset(buf, uint8_t *, 14);
            rte_ether_addr_copy(&address.mac, &ether_hdr->src_addr);
            ids[1] = address.id;
            rte_ether_addr_copy(&server_address.mac, &ether_hdr->dst_addr);
            ids[0] = server_address.id;
            ether_hdr->ether_type = rte_cpu_to_be_16(0x88d5);
            ret = rte_eth_tx_burst(port_id, 0, &buf, 1);
            assert(ret == 1);
        }
    }

    for (;;)
    {
        struct rte_mbuf *bufs[32];
        const uint16_t n_rx = rte_eth_rx_burst(port_id, 0, bufs, 32);
        for (uint16_t i = 0; i < n_rx; i += 1)
        {
            struct rte_mbuf *buf = bufs[i];
            struct rte_ether_hdr *ether_hdr = rte_pktmbuf_mtod(buf, struct rte_ether_hdr *);
            uint8_t *ids = rte_pktmbuf_mtod_offset(buf, uint8_t *, 14);
            if (!rte_is_same_ether_addr(&ether_hdr->dst_addr, &address.mac) || ids[0] != address.id)
            {
                rte_pktmbuf_free(buf);
                continue;
            }

            struct rte_mbuf *tx_buf = rte_pktmbuf_alloc(mbuf_pool);
            tx_buf->pkt_len = tx_buf->data_len = 16;
            struct rte_ether_hdr *tx_ether_hdr = rte_pktmbuf_mtod(tx_buf, struct rte_ether_hdr *);
            uint8_t *tx_ids = rte_pktmbuf_mtod_offset(tx_buf, uint8_t *, 14);
            rte_ether_addr_copy(&ether_hdr->dst_addr, &tx_ether_hdr->src_addr);
            tx_ids[1] = ids[0];
            rte_ether_addr_copy(&ether_hdr->src_addr, &tx_ether_hdr->dst_addr);
            tx_ids[0] = ids[1];
            rte_pktmbuf_free(buf);

            tx_ether_hdr->ether_type = rte_cpu_to_be_16(0x88d5);
            ret = rte_eth_tx_burst(port_id, 0, &tx_buf, 1);
            assert(ret == 1);

            rte_atomic32_inc(&count);
        }
    }

    return 0;
}