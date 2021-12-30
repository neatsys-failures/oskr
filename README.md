## High Performance Distributed Protocols Collection
![GitHub repo size](https://img.shields.io/github/repo-size/sgdxbc/oskr)
![GitHub code size in bytes](https://img.shields.io/github/languages/code-size/sgdxbc/oskr)
![Lines of code](https://img.shields.io/tokei/lines/github/sgdxbc/oskr)
![GitHub contributors](https://img.shields.io/github/contributors/sgdxbc/oskr)
![GitHub commit activity](https://img.shields.io/github/commit-activity/m/sgdxbc/oskr)

**Motivation.** This is an attempt to improve based on [specpaxos]. The detail 
of consideration is listed in [a piece of blog][sgd-blog].

**Why named Oskr?** The name is derived from the Oscars (Academy Awards), 
because the core of this project is based on a specialized actor model.

**Present issues:**
* Heavily-used template programming + self-contained header result in terrible 
  error reporting and long compilation time. Because most class in header are 
  templated, precompiled header not help much.
  * Template programming also make interface more cumbersome with `typename` and
    such.
* Dependencies:
  * Using Folly in serveral places, which is hard to set up and currently 
    requires hacking.
  * Serialization leverage Bitsery, which is a little bit lack of document.
* CMake makes me feel good, meson makes me feel better, but at the end of the
  day I have to use CMake because of the supportness of most dependencies. This
  causes the setup of DPDK a little bit hacky and maybe fragile.

**Roadmap:**
- [ ] Architecture design + Viewstamped Replication
- [ ] PBFT
- [ ] HotStuff
- [ ] Two-phase commit

[specpaxos]: https://github.com/UWSysLab/specpaxos
[sgd-blog]: https://sgdxbc.github.io/ideas/2021-12-15/p0

----

Develop on Ubuntu 21.10, with Clang version 13. Required apt packages (can be
installed with `--no-install-recommends`):

```
General building:
    cmake clang clang-tidy
For DPDK:
    pkg-config meson ninja-build python3-pyelftools 
For Folly:
    libboost-context-dev libboost-filesystem-dev libboost-program-options-dev
    libboost-regex-dev libboost-system-dev libboost-thread-dev 
    libdouble-conversion-dev libevent-dev libfmt-dev libgoogle-glog-dev 
    libssl-dev
```

Step 1, clone the repo with `--recursive`.

Step 2, go to `dependency/folly` and run
```
git apply ../folly-has_coroutine_check.patch
```

Step 3, build CMake project as usual. Initial configuration will build a DPDK, 
which takes some time. Notable targets:
* `Test*` unit tests (to get full list run `make help | grep run-Test`)
  * `run-Test*` to run one test unit
  * `run-test` to run all test units
* `Client` benchmark client, executable at `<build>/benchmark/Client`.

*Work in progress.*

----

Project structure:
* `core`<sup>&dagger;</sup> behavioral definition of abstraction components. Pure interfaces
  without concrete logic.
  * `Foundation.hpp` an all-in-one header for protocol implementation.
* `common`<sup>&dagger;</sup> common behavior logic prepared for reusing.
* `app`<sup>&dagger;</sup> builtin applications to be supported by protocols.
* `transport`<sup>&dagger;</sup> runtime implementations that support protocols.
* `replication` replication protocols.
* `transactional` transactional protocols.
* `dependency` git submodule stubs.
* `test` flat directory for tests.
* `benchmark` universal entry executable for running all protocols.
* `doc` out-of-source documents and format configuration.

<span>&dagger;</span> Document of these source is hosted on [project site][site].

[site]: https://sgdxbc.github.io/oskr
