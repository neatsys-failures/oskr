### High Performance Distributed Protocols Collection

**Motivation.** This is an attempt to improve based on [specpaxos]. The detail 
of consideration is listed in [a piece of blog][sgd-blog].

**Why named Oscar?** Because the core of this project is based on a specialized 
actor model.

**Present issues.**
* Heavily-used template programming causing terrible error reporting.
* Heavily-used template programming + self-contained header result in long 
  compilation time. Because most class in header are templated, precompiled
  header not help much.
* Serialization leverage Bitsery, which is a little bit lack of document.

[specpaxos]: https://github.com/UWSysLab/specpaxos
[sgd-blog]: https://sgdxbc.github.io/ideas/2021-12-15/p0

----

*Work in progress.*
