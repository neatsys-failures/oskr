## High Performance Distributed Protocols Collection
![Crates.io](https://img.shields.io/crates/v/oskr)
![Crates.io](https://img.shields.io/crates/l/oskr)
![docs.rs](https://img.shields.io/docsrs/oskr)
![Lines of code](https://img.shields.io/tokei/lines/github/sgdxbc/oskr)
![GitHub contributors](https://img.shields.io/github/contributors/sgdxbc/oskr)
![GitHub commit activity](https://img.shields.io/github/commit-activity/m/sgdxbc/oskr)

**Motivation.** This is an attempt to improve based on [specpaxos]. Notice that
although the project is titled *high performance*, we don't do obscure 
optimization on propose, especially all included protocol implementations are 
canonical. The high performance mainly means that the project provides a stage 
interface for replicas, so they can efficiently utilize multi-processor system.

**Why named Oskr?** The name is derived from the Oscars (Academy Awards), 
because the core of this project is based on a specialized actor model.

[specpaxos]: https://github.com/UWSysLab/specpaxos

**Benchmark result.** Detailed explaination work in progress.

|Protocol      |Worker number|Batch size|Maximum throughput (K ops/sec)|Minimum medium latency (us)|
|--------------|-----|-----|-----------|-----------|
|Unreplicated  |1    |1    |1901.157   |12.031     |
|PBFT          |14   |1    |14.686     |716.799    |
|PBFT          |14   |100  |100.2*     |1449.983   |
|HotStuff      |

----

**Setup.** To run this project you must compile from source. There is a 
published version on crate.io, but that is just a placeholder and will not work.
The propose is to enable hosting document on doc.rs.

Prerequisites on Ubuntu:
* Up-to-date stable Rust toolchain.
* For compiling DPDK: any working C building toolchain, `meson`, `ninja-build`,
`python3-pyelftools`.


1. Clone repository recursively.
2. Compile DPDK: `meson setup target/dpdk src/dpdk && ninja -C target/dpdk`.
3. To run unit tests: `cargo test --lib`.
4. To build benchmark executables: `cargo build --release`.

   The compiled executables are dynamically linked to rte shared objects, so if 
   you transfer executables to a remote machine to run, make sure `target/dpdk` 
   exists in remote working directory.
5. Create deploy configuration. Create description file `deploy/shard0.config`,
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
6. Create signing key files. Generate signing key file for replica 0 with:
   ```
   openssl ecparam -genkey -noout -name secp256k1 | openssl pkcs8 -topk8 -nocrypt -out deploy/shard0-0.pem
   ```
   Run the command three more times to generate `deploy/shard0-{1,2,3}.pem`.
7. Start replica 0 with:
   ```
   sudo ./target/release/replica -m pbft -c deploy/shard0 -i 0
   ```
   Then start replica 1, 2 and 3 with corresponding `-i` option on the servers
   assigned to them.
8. Start client with:
   ```
   sudo ./target/release/client -m pbft -c deploy/shard0
   ```
   You may use `-t` to spawn multiple clients that send concurrent requests, or
   use `-d` to extend sending duration.

Kindly pass `-h` to executables to learn about other options.