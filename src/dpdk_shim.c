#include <rte_mbuf.h>
#include <rte_ethdev.h>

uint8_t *mbuf_to_buffer_data(struct rte_mbuf *mbuf) {
    return rte_pktmbuf_mtod_offset(mbuf, uint8_t *, 16);
}

void *buffer_data_to_mbuf(uint8_t *data) {
    return data; // TODO
}

uint16_t mbuf_get_packet_length(struct rte_mbuf *mbuf) {
    return mbuf->pkt_len;
}

void mbuf_set_packet_length(struct rte_mbuf *mbuf, uint16_t length) {
    mbuf->pkt_len = length;
}

uint16_t oskr_eth_rx_burst(uint16_t port_id, uint16_t queue_id, struct rte_mbuf **rx_pkts, uint16_t nb_pkts) {
    return rte_eth_rx_burst(port_id, queue_id, rx_pkts, nb_pkts);
}

uint16_t oskr_eth_tx_burst(uint16_t port_id, uint16_t queue_id, struct rte_mbuf **tx_pkts, uint16_t nb_pkts) {
    return rte_eth_tx_burst(port_id, queue_id, tx_pkts, nb_pkts);
}

struct rte_mbuf *oskr_pktmbuf_alloc(struct rte_mempool *mp) {
    return rte_pktmbuf_alloc(mp);
}

void oskr_pktmbuf_free(struct rte_mbuf *m) {
    rte_pktmbuf_free(m);
}

uint16_t oskr_mbuf_default_buf_size() {
    return RTE_MBUF_DEFAULT_BUF_SIZE;
}