<!-- prevent doxygen adapt title from below -->
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
  error reporting long compilation time. Because most class in header are 
  templated, precompiled header not help much.
  * Template programming also make interface more cumbersome with `typename` and
    such.
* Serialization leverage Bitsery, which is a little bit lack of document.
* CMake makes me feel good, meson makes me feel better, but at the end of the
  day I have to use CMake because of the supportness of most dependencies. This
  causes the setup of DPDK a little bit hacky and maybe fragile.
* The *executors* draft is not standardized yet. Once it 
  is done, certain part of the project (i.e. `Transport`) may be invalidated by 
  it:
  * For non-DPDK `Transport`, standard may provide a better alternative but the
    migration could be hard.
  * Althrough standard or anyone else probably will not provide DPDK 
    `Transport`s, its non-standard shape may impede other projects to adapt it.

**Roadmap:**
- [ ] Architecture design + Viewstamped Replication
- [ ] PBFT
- [ ] HotStuff
- [ ] Two-phase commit

[specpaxos]: https://github.com/UWSysLab/specpaxos
[sgd-blog]: https://sgdxbc.github.io/ideas/2021-12-15/p0

----

Develop on Ubuntu 21.10, with Clang version 13. Required apt packages:

```
clang cmake pkg-config meson ninja-build python3-pyelftools libboost-dev clang-tidy
```

Step 1, clone the repo with `--recursive`.

Step 2, build CMake project as usual. Initial configuration will build a DPDK, 
which takes some time. Notable targets:
* `Client` benchmark client, executable at `<build>/benchmark/Client`.
* `Test*` unit tests (to get full list run `make help | grep Test`), executable
  at `<build>/test/Test*`

*Work in progress.*

----

Project structure:
* `core`[^1] behavioral definition of abstraction components. Pure interfaces
  without concrete logic.
  * `Foundation.hpp` an all-in-one header for protocol implementation.
* `common`[^1] common behavior logic prepared for reusing.
* `app`[^1] builtin applications to be supported by protocols.
* `transport`[^1] runtime implementations that support protocols.
* `replication` replication protocols.
* `transactional` transactional protocols.
* `dependency` git submodule stubs.
* `test` flat directory for tests.
* `benchmark` universal entry executable for running all protocols.

[^1]: Document of these source is hosted on [project site][site].

[site]: https://sgdxbc.github.io/oskr
