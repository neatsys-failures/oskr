<div align="center">
<img src="./logo-banner.svg" height=200>

## High Performance Distributed Works Collection

![Crates.io](https://img.shields.io/crates/v/oskr)
![Crates.io](https://img.shields.io/crates/l/oskr)
![docs.rs](https://img.shields.io/docsrs/oskr)
![Lines of code](https://img.shields.io/tokei/lines/github/sgdxbc/oskr)
![GitHub contributors](https://img.shields.io/github/contributors/sgdxbc/oskr)
![GitHub commit activity](https://img.shields.io/github/commit-activity/m/sgdxbc/oskr)
</div>

**Motivation.** This is an attempt to improve based on [specpaxos]. 
Traditionally system work is hard to evaluate and compare, because it takes a
lot of effort to reimplement all considered works into same comparable model,
while directly run their original codebases usually cannot give us comparable
results. Similar to specpaxos this codebase provide a general foundation for
reimplement and evaluate, with several implementation of popular works out of
the box, as a solution for the first approach.

To keep fairness and as simple as possible, the codebase intentionally avoids
too specific optimizations in framework. For example all dependencies are either
standard library or standard community choice. This is a codebase with a high
performance architecture and abstraction, and a straightforward reference
implementation to it.

[specpaxos]: https://github.com/UWSysLab/specpaxos

**Why named Oskr?** The name is derived from the Oscars (Academy Awards), 
because the core of this project is based on a specialized actor model.

It is also abbreviation for Overengineered System from Kent Ridge (more 
precisely, 117418 Singapore), and **o**range **s**a**k**u**r**a which is
project's logo.

**Benchmark result.** Detailed explaination work in progress.

0/0 RPC without batching

|Protocol|Worker number|Maximum throughput (Kops/sec)|Minimum medium latency (us)|
|-------------------|---|-----------|-----------|
|Unreplicated       |1  |2011.256   |10.751     |
|Unreplicated Signed|1  |11.799     |91.647     |
|Unreplicated Signed|30 |327.876    |99.327     |
|PBFT               |30 |8.060      |724.991    |
|HotStuff           |30 |1.833      |2185.454   |
|Zyzzyva            |30 |135.900    |348.159    |

0/0 RPC with batch size up to 100

|Protocol|Adaptive batching|Maximum throughput (Kops/sec)|
|-------------------|---|-----------|
|PBFT               |Yes|221.189    |
|HotStuff           |Yes|147.704    |
|Zyzzyva            |No |243.387    |

YCSB workload C with in-memory B-Tree map as backend, 1M 10*64B records, batch size up to 20

|Protocol|Maximum throughput (Kops/sec)|Minimum latency (us)|
|-------------------|-------|-----------|
|Unreplicated       |80.788 |31.999     |
|Unreplicated Signed|88.268 |121.855    |
|Zyzzyva            |89.080 |362.495    |
|PBFT               |47.367 |737.279    |
|HotStuff           |35.180 |2211.839   |

----

**Setup.** To run this project you must compile from source. There is a 
published version on crates.io, but that is just a placeholder and will not 
work. The propose is to enable hosting document on docs.rs.

Prerequisites on Ubuntu:
* Up-to-date stable Rust toolchain.
* For compiling DPDK: any working C building toolchain, `meson`, `ninja-build`,
`python3-pyelftools`.


1.  Clone repository recursively.
2.  Compile DPDK: `meson setup target/dpdk src/dpdk && ninja -C target/dpdk`.
3.  To run unit tests: `cargo test --lib`.
4.  To build benchmark executables: `cargo build --release`.

    The compiled executables are dynamically linked to rte shared objects, so if 
    you transfer executables to a remote machine to run, make sure `target/dpdk` 
    exists in remote working directory.
5.  Create deploy configuration. Create description file `deploy/quad.config`,
    write the following lines:
    ```
    f 1
    replica 12:34:56:aa:aa:aa%0
    replica 12:34:56:bb:bb:bb%0
    replica 12:34:56:cc:cc:cc%0
    replica 12:34:56:dd:dd:dd%0
    multicast 01:00:5e:00:00:01%255
    ```
    Replace MAC addresses with the ones of network cards, and make sure network
    is able to do L2 forward for packets sent to these MAC addresses. The 
    multicast line is optional for running PBFT.
6.  Create signing key files. Generate signing key file for replica 0 with:
    ```
    openssl ecparam -genkey -noout -name secp256k1 | openssl pkcs8 -topk8 -nocrypt -out deploy/quad-0.pem
    ```
    Run the command three more times to generate `deploy/quad-{1,2,3}.pem`.
7.  Start replica 0 with:
    ```
    sudo ./target/release/replica -m pbft -c deploy/quad -i 0
    ```
    Then start replica 1, 2 and 3 with corresponding `-i` option on the servers
    assigned to them.
8.  Start client with:
    ```
    sudo ./target/release/client -m pbft -c deploy/quad
    ```
    You may use `-t` to spawn multiple clients that send concurrent requests, or
    use `-d` to extend sending duration.

Kindly pass `-h` to executables to learn about other options.