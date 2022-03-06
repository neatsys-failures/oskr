## High Performance Distributed Protocols Collection
![GitHub repo size](https://img.shields.io/github/repo-size/sgdxbc/oskr)
![GitHub code size in bytes](https://img.shields.io/github/languages/code-size/sgdxbc/oskr)
![Lines of code](https://img.shields.io/tokei/lines/github/sgdxbc/oskr)
![GitHub contributors](https://img.shields.io/github/contributors/sgdxbc/oskr)
![GitHub commit activity](https://img.shields.io/github/commit-activity/m/sgdxbc/oskr)

**Motivation.** This is an attempt to improve based on [specpaxos].

**Why named Oskr?** The name is derived from the Oscars (Academy Awards), 
because the core of this project is based on a specialized actor model.

[specpaxos]: https://github.com/UWSysLab/specpaxos

**Benchmark result.** Work in progress.

----

**Setup.** Prerequisites on Ubuntu:
* Up-to-date stable Rust toolchain.
* For compiling DPDK: any working C building toolchain, `meson`, `ninja-build`,
`python3-pyelftools`.


1. Clone repository recursively.
2. Compile DPDK: `meson setup target/dpdk src/dpdk && ninja -C target/dpdk`.
3. To run unit tests: `cargo test --lib`.
4. To build benchmark executables: `cargo build --release`.

To run benchmark, `target/dpdk` must exist in working directory. More
instruction work in progress.