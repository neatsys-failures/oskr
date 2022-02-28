#include <rte_mbuf.h>
#include <rte_ethdev.h>

uint8_t *mbuf_get_data(struct rte_mbuf *mbuf)
{
    return rte_pktmbuf_mtod(mbuf, uint8_t *);
}

uint16_t mbuf_get_packet_length(struct rte_mbuf *mbuf)
{
    return mbuf->pkt_len;
}

void mbuf_set_packet_length(struct rte_mbuf *mbuf, uint16_t length)
{
    mbuf->data_len = mbuf->pkt_len = length;
}

uint16_t oskr_eth_rx_burst(uint16_t port_id, uint16_t queue_id, struct rte_mbuf **rx_pkts, uint16_t nb_pkts)
{
    return rte_eth_rx_burst(port_id, queue_id, rx_pkts, nb_pkts);
}

uint16_t oskr_eth_tx_burst(uint16_t port_id, uint16_t queue_id, struct rte_mbuf **tx_pkts, uint16_t nb_pkts)
{
    return rte_eth_tx_burst(port_id, queue_id, tx_pkts, nb_pkts);
}

struct rte_mbuf *oskr_pktmbuf_alloc(struct rte_mempool *mp)
{
    return rte_pktmbuf_alloc(mp);
}

void oskr_pktmbuf_free(struct rte_mbuf *m)
{
    rte_pktmbuf_free(m);
}

uint16_t oskr_mbuf_default_buf_size()
{
    return RTE_MBUF_DEFAULT_BUF_SIZE;
}

int setup_port(uint16_t port_id, uint16_t n_rx, uint16_t n_tx, struct rte_mempool *pktmpool)
{
    struct rte_eth_conf port_conf;
    memset(&port_conf, 0, sizeof(struct rte_eth_conf));
    port_conf.txmode.mq_mode = ETH_MQ_TX_NONE;

    struct rte_eth_dev_info dev_info;
    if (rte_eth_dev_info_get(port_id, &dev_info) != 0)
    {
        return -1;
    }

    if (dev_info.tx_offload_capa & DEV_TX_OFFLOAD_MBUF_FAST_FREE)
    {
        port_conf.txmode.offloads |= DEV_TX_OFFLOAD_MBUF_FAST_FREE;
    }

    if (rte_eth_dev_configure(port_id, n_rx, n_tx, &port_conf) != 0)
    {
        return -1;
    }

    uint16_t n_rx_desc = 4096, n_tx_desc = 4096;
    if (rte_eth_dev_adjust_nb_rx_tx_desc(port_id, &n_rx_desc, &n_tx_desc) != 0)
    {
        return -1;
    }

    struct rte_eth_rxconf rxconf = dev_info.default_rxconf;
    rxconf.offloads = port_conf.rxmode.offloads;
    for (int i = 0; i < n_rx; i += 1)
    {
        if (rte_eth_rx_queue_setup(
                port_id, i, n_rx_desc, rte_eth_dev_socket_id(port_id), &rxconf,
                pktmpool) != 0)
        {
            return -1;
        }
    }

    struct rte_eth_txconf txconf = dev_info.default_txconf;
    txconf.offloads = port_conf.txmode.offloads;
    for (int i = 0; i < n_tx; i += 1)
    {
        if (rte_eth_tx_queue_setup(
                port_id, i, n_tx_desc, rte_eth_dev_socket_id(port_id), &txconf) != 0)
        {
            return -1;
        }
    }

    if (rte_eth_dev_start(port_id) != 0)
    {
        return -1;
    }

    if (rte_eth_promiscuous_enable(port_id) != 0)
    {
        return -1;
    }

    // add flow rules when necessary
    return 0;
}

void print_errno() {
    printf("%s\n", rte_strerror(rte_errno));
}