### High Performance Distributed Protocols Collection

**Motivation.** This is an attempt to improve based on [specpaxos]. The detail 
of consideration is listed in [a piece of blog][sgd-blog].

**Why named Oscar?** Because the core of this project is based on a specialized 
actor model.

**Present issues:**
* Heavily-used template programming causing terrible error reporting.
* Heavily-used template programming + self-contained header result in long 
  compilation time. Because most class in header are templated, precompiled
  header not help much.
* Serialization leverage Bitsery, which is a little bit lack of document.
* Some part of interface has cumbersome syntax, because of required `typename`
  and such.

[specpaxos]: https://github.com/UWSysLab/specpaxos
[sgd-blog]: https://sgdxbc.github.io/ideas/2021-12-15/p0

----

Develop on Ubuntu 21.10, with clang version 13. Required apt packages:
* `cmake`
* `clang`
* `clang-tidy`
* `libboost-dev`

Step 1, clone the repo with `--recursive`.

Step 2, build CMake project as usual.

*Work in progress.*

----

Project structure:
* `core`[^1] classes designed to be inherited, and something essential to those 
  classes.
  * `Foundation.hpp` an all-in-one header for protocol implementation.
* `common`[^1] classes designed to be composed.
* `app`[^1] builtin applications to be supported by protocols.
* `transport`[^1] runtime implementations that support protocols.
* `replication` replication protocols.
* `transactional` transactional protocols.
* `dependency` git submodule stubs.
* `test` flat directory for tests.

[^1]: Document of these source is hosted on [project site][site].

[site]: https://sgdxbc.github.io/oscar
