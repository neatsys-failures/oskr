## High Performance Distributed Protocols Collection
![GitHub repo size](https://img.shields.io/github/repo-size/sgdxbc/oskr)
![GitHub code size in bytes](https://img.shields.io/github/languages/code-size/sgdxbc/oskr)
![Lines of code](https://img.shields.io/tokei/lines/github/sgdxbc/oskr)
![GitHub contributors](https://img.shields.io/github/contributors/sgdxbc/oskr)
![GitHub commit activity](https://img.shields.io/github/commit-activity/m/sgdxbc/oskr)

**Motivation.** This is an attempt to improve based on [specpaxos]. Notice that
although the project is titled *high performance*, we don't do obscure 
optimization on propose, especially there is no custom optimization in protocol
implementations. The high performance mainly means that the project provides a
stage interface for replicas, so they can efficiently utilize multi-processor
system.

**Why named Oskr?** The name is derived from the Oscars (Academy Awards), 
because the core of this project is based on a specialized actor model.

[specpaxos]: https://github.com/UWSysLab/specpaxos

**Benchmark result.** Work in progress.

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

5. Create deploy configuration. A configuration consists a description file and
   a set of signing key files. Both files should contain same prefix in their
   names, for example `deploy/shard0`. The description file 
   `deploy/shard0.config` has following lines:

   ```
   f <number of fault nodes to tolerance>
   replica <address>
   replica <address>
   ...
   [multicast <address>]
   ```

   The last line is only for configuration that contains a multicast address.

6. Create signing key files. Each replica presents in description file above
   must own a secp256k1 signing key, stores in `deploy/shard0-<i>.pem`, where
   `<i>` is replica's id starts from 0. You can generate signing key file with
   following command:

   ```
   openssl ecparam -genkey -noout -name secp256k1 | openssl pkcs8 -topk8 -nocrypt -out deploy/shard0-0.pem
   ```

Now you are ready to run replica and client executables. Their command line
options are self-contained documented, and are omitted here.
