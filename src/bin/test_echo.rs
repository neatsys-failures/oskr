use std::{
    env,
    ffi::CString,
    mem::MaybeUninit,
    os::raw::{c_char, c_int},
    ptr::NonNull,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
    thread::{sleep, spawn},
    time::Duration,
};

use oskr::dpdk_shim::{
    oskr_eth_rx_burst, oskr_eth_tx_burst, oskr_mbuf_default_buf_size, oskr_pktmbuf_alloc,
    oskr_pktmbuf_free, rte_eal_init, rte_eth_dev_socket_id, rte_mbuf, rte_pktmbuf_pool_create,
    setup_port, Address,
};

fn main() {
    let server_address: Address = "b8:ce:f6:2a:2f:94#0".parse().unwrap();
    let port_id = 0;
    let invoke = env::args()
        .nth(1)
        .map(|arg| arg == "invoke")
        .unwrap_or(false);

    let prog = env::args().nth(0).unwrap();
    let args = [&prog, "-c", "0x01"];
    let args: Vec<_> = args
        .into_iter()
        .map(|arg| CString::new(arg).unwrap())
        .collect();
    let mut args: Vec<_> = args
        .iter()
        .map(|arg| NonNull::new(arg.as_ptr() as *mut c_char).unwrap())
        .collect();
    let ret = unsafe {
        rte_eal_init(
            args.len() as c_int,
            NonNull::new(&mut args as &mut [_] as *mut [_] as *mut _).unwrap(),
        )
    };
    assert_eq!(ret, args.len() as c_int - 1);

    let address = if invoke {
        unsafe { Address::new_local(port_id, 254) }
    } else {
        server_address
    };

    let name = CString::new("MBUF_POOL").unwrap();
    let name = NonNull::new(&name as *const _ as *mut _).unwrap();
    let pktmpool = NonNull::new(unsafe {
        rte_pktmbuf_pool_create(
            name,
            8191,
            250,
            0,
            oskr_mbuf_default_buf_size(),
            rte_eth_dev_socket_id(port_id),
        )
    })
    .unwrap();
    let ret = unsafe { setup_port(port_id, 1, 1, pktmpool) };
    assert_eq!(ret, 0);

    if invoke {
        for _ in 0..10 {
            let mbuf = NonNull::new(unsafe { oskr_pktmbuf_alloc(pktmpool) }).unwrap();
            let data = unsafe { rte_mbuf::get_data(mbuf) };
            unsafe {
                rte_mbuf::set_source(data, &address);
                rte_mbuf::set_dest(data, &server_address);
                rte_mbuf::set_buffer_length(mbuf, 0);
            }
            let ret = unsafe {
                oskr_eth_tx_burst(
                    port_id,
                    0,
                    NonNull::new(&mbuf as *const _ as *mut _).unwrap(),
                    1,
                )
            };
            assert_eq!(ret, 1);
        }
    }

    let count = Arc::new(AtomicU32::new(0));
    spawn((|| {
        let count = count.clone();
        move || loop {
            sleep(Duration::from_secs(1));
            let count = count.swap(0, Ordering::SeqCst);
            println!("{}", count);
        }
    })());

    loop {
        let mut burst: MaybeUninit<[*mut rte_mbuf; 32]> = MaybeUninit::uninit();
        let burst_size = unsafe {
            oskr_eth_rx_burst(
                port_id,
                0,
                NonNull::new(burst.as_mut_ptr() as *mut _).unwrap(),
                32,
            )
        };
        let burst = &unsafe { burst.assume_init() }[..burst_size as usize];
        for mbuf in burst {
            let mbuf = NonNull::new(*mbuf).unwrap();
            let data = unsafe { rte_mbuf::get_data(mbuf) };
            let (source, dest) = unsafe { (rte_mbuf::get_source(data), rte_mbuf::get_dest(data)) };
            unsafe { oskr_pktmbuf_free(mbuf) };
            if dest != address {
                continue;
            }

            let mbuf = NonNull::new(unsafe { oskr_pktmbuf_alloc(pktmpool) }).unwrap();
            let data = unsafe { rte_mbuf::get_data(mbuf) };
            unsafe {
                rte_mbuf::set_source(data, &dest);
                rte_mbuf::set_dest(data, &source);
                rte_mbuf::set_buffer_length(mbuf, 0);
            }
            let ret = unsafe {
                oskr_eth_tx_burst(
                    port_id,
                    0,
                    NonNull::new(&mbuf as *const _ as *mut _).unwrap(),
                    1,
                )
            };
            assert_eq!(ret, 1);
            count.fetch_add(1, Ordering::SeqCst);
        }
    }
}
