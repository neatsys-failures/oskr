//! High performance distributed protocols collection.
//!
//! The detail implementation of various protocols and applications are mostly
//! undocumented, refer to original work for them.
//!
//! The document here mainly for:
//! * Instruction on how to implement protocols on top of provided runtime.
//!   Check [`protocol::unreplicated`] module for a beginner example.
//! * Instruction on how to evaluate with this codebase. Check provided binaries
//!   for reference.
//! * Record some explanation of design choice, help me think consistently over
//!   long develop period.
//!
//! # Stability
//!
//! As the time of writing this, we are around release candidate of 1.0 version,
//! and I have tried out most alternative architecture and components, and I
//! believe that most thing remain here comes with a reason.
//!
//! As a result, hopefully there will be no major breaking update on the
//! codebase, i.e. everything in [`facade`] module remains the same forever. The
//! future work should be:
//! * Add more protocols and applications implementation and evaluate them.
//! * Add more runtime facilities, e.g. kernel network stack, if necessary.
//! * Bump toolchain and dependencies version.

/// Interfaces across top-level modules, and to outside.
///
/// The general architecture follows [specpaxos], with following mapping:
/// * `Transport`: [`Transport`](facade::Transport) and
///   [`TxAgent`](facade::TxAgent)
/// * `TransportReceiver`: [`Receiver`](facade::Receiver) and `rx_agent` closure
/// * `Configuration`: [`Config`](facade::Config)
/// * `TransportAddress`: [`Transport::Address`](facade::Transport::Address)
/// * `AppReplica`: [`App`](facade::App)
/// * `Client`: [`Invoke`](facade::Invoke)
///
///   (There is nothing corresponding to `Replica` right now, replica receivers
///   interact with applications directly.)
///
/// [specpaxos]: https://github.com/UWSysLab/specpaxos
///
/// There is some modification to allow us work with Rust's borrow and lifetime
/// system, but all implementations' code should be able to be organized in the
/// same way as specpaxos.
///
/// Additionally, [`AsyncEcosystem`](facade::AsyncEcosystem) trait allow
/// receiver to work in asynchronized way, which is probably required by all
/// `Invoke`able receivers. The multithreading counterpart [`stage`] is designed
/// as a fixed-implementation module, and stay outside of the facade.
pub mod facade;

/// Low-level DPDK binding.
///
/// For practical usage consider [`framework::dpdk`], which is a higher level
/// DPDK interface based on this module.
///
/// This shim module contains two parts: a set of `extern "C"` DPDK function
/// definitions, and a custom L2 packet layout.
///
/// The extern functions starts with `rte_` present in DPDK's shared objects,
/// and will be linked directly. The other functions which starts with `oskr_`
/// are static inline DPDK functions that defined in C header files, so this
/// codebase create a custom stub in `dpdk_shim.c` to be linked against.
///
/// There is also a `setup_port` function, which underly calls several DPDK
/// functions to set up a ethernet device port properly.
///
/// # DPDK transport packet format
///
/// By default DPDK does not perform TCP/IP network stack processing, so if we
/// don't include these protocol layers, the packet parsing/assembling part can
/// be omitted, and the transport packets are smaller.
///
/// The DPDK transport packet layout takes 17 bytes. The first 14 bytes are
/// normal ethernet layer: 6 bytes destination MAC address, 6 bytes source MAC
/// address, and 2 bytes ethernet type is specified as `0x88d5`. Then there are
/// 1 byte destination local id and 1 byte source local id. Finally, there is 1
/// byte as id extension field. Local id pairs are encoded as following:
/// * The local id field contains the low 7 bits.
/// * If the highest bit of local id field is 1, then the id is a 15 bits
///   "large" id, and the extension field contains high 8 bits. Otherwise, it is
///   a 7 bits "small" id.
/// * At most one local id of the two is large. If both of them are small,
///   extension field is unused.
///
/// The transport packet format can be transfered with normal L2 forwarding, and
/// support up to 128 local ids on small side, and up to 32768 local ids on
/// large side. This allow users to run large number of clients from the same
/// machine. Notice that the format actually allows more than 128 servers, i.e.
/// well-known addresses. Just use difference MAC addresses. The format is also
/// capatible with L2 multicast.
///
/// The related accessing and manipulating functions of the packet format is
/// defined in [`rte_mbuf`](dpdk_shim::rte_mbuf).
pub mod dpdk_shim;

/// Simulated facilities for writing test cases.
#[cfg(any(test, doc))]
pub mod simulated;

/// Stage abstraction. Receiver on stage can use multiple threads efficiently.
///
/// Strictly speaking this is part of [`framework`]. Keeping in its own module
/// to emphasize its importance.
///
/// # Why create another wheel?
///
/// There are good enough thread pool libraries from Rust community. [Rayon] for
/// example is one of the most popular, and it is able to be used in this
/// codebase. Alternatively, the async-based libraries are also good for
/// multithreading in some ways.
///
/// [Rayon]: https://docs.rs/rayon/latest/rayon/
///
/// Building a new stage is not for extreme performance, and there is no plan to
/// perform special optimization from the first place. The requirement is more
/// about a specialized interface, which:
/// * Makes a distinguishment between *stateful* tasks and *stateless* tasks.
///   Most protocol implementations are centralized to "one big state", and
///   tranditionally they runs in a single-threaded context.
///
///   The meaning of stage is to annotate stateless snippets, give runtime a
///   chance to go concurrent and dramatically increase performance. This is
///   conceptually different to existed worker pool models, which assume most
///   tasks are independent and treat stateful tasks as exception.
///
///   In practice, this stage module puts minimal affection on implementation's
///   logical structure. No lock or channel is required.
/// * Provides task priority. Certain protocols may rely on dynamical priority
///   semantic, i.e. determine task-processing order upon submitting, which
///   cannot be expressed as simple FIFO or LIFO strategy.
/// * Provides timing task. This interface is absent in rayon, and although it
///   presents in async libraries, it may not guarantee efficient implementation
///   of reseting deadline, which is widely required by view change related
///   timers.
///
/// Additionally, providing a standard stage for server-side receiver serves as
/// a supplement to the *lightweight RX agent* rule of
/// [`Transport`](facade::Transport). It is recommended for all server-side
/// receiver to:
/// * Be on stage.
/// * Submit as soon as possible in RX agent.
///
/// # Why conditional compiling?
///
/// In this codebase we try to avoid conditional compiling in most places.
/// Instead, we abstract aspects of runtime into traits in [`facade`]. However,
/// the stage module is using conditional compiling: in test profile the stage
/// module is mocked by an asynchronized polyfill from [`simulated`] module.
///
/// The reason for this design is mostly because I believe there will be only
/// one "suitable" stage implementation for all scenario except testing.
/// Opposite to this, transport may be DPDK-based or kernel-based, and async
/// ecosystem that based on Tokio or smol may not be suitable for benchmark.
/// Making abstraction is fun, but more abstraction means more learning efforts
/// users need to spend, so skipping should be better.
///
/// **The shortage of conditional compiling.** The polyfill simulated
/// implementation only promises to work when pairing with async ecosystem from
/// [`framework::tokio`]. Because of conditional compiling there is no way to
/// force this pairing (the generic specialization of Rust is still on its way).
///
/// # Why callback hell?
///
/// The [`Submit`](stage::Submit), core interface of stage module, cause
/// submitted task one level deeper in closure. There are lots of efforts on
/// avoid such callback hell, such as promise and async syntax. These solutions
/// normally only works well on sequential logic. However, implementations
/// usually do conditionally submitting, or even submit in a loop, for example
/// sending replies while executing a batch. As far as I know, callback closure
/// is still the most nature way to express in such case.
///
/// # Example: sample sort
///
/// ```
/// # use std::{sync::{Arc, atomic::{AtomicBool, Ordering}}, iter::once};
/// # use oskr::stage::{Handle, Submit, State};
/// # use rand::{Rng, thread_rng, distributions::Uniform, seq::SliceRandom};
/// // sort list of u32 and merge it into state (only work for once)
/// // invariant: state vec is always sorted
/// struct SortingState(Vec<u32>);
/// 
/// impl State for SortingState {
///     type Shared = (); // nothing need to be shared here
///     fn shared(&self) -> Self::Shared {}
/// }
/// 
/// // these are free functions because document code example locate outside
/// // crate. normally they could be `StatelessContext<SortingState>`'s methods
/// fn sort(submit: &Submit<SortingState>, list: Vec<u32>, p: usize, completed: Arc<AtomicBool>) {
///     let n = list.len();
///     submit.stateless(move |shared| sort_internal(&shared.submit, list, p, n, completed));
/// }
/// 
/// fn sort_internal(
///     submit: &Submit<SortingState>,
///     mut list: Vec<u32>,
///     p: usize,
///     n: usize,
///     completed: Arc<AtomicBool>,
/// ) {
///     if list.is_empty() {
///         return;
///     }
///     // hardcoded small sort threshold for simplicity
///     if list.len() <= 32 {
///         list.sort_unstable();
///         submit.stateful(move |state| {
///             let position = state
///                 .0
///                 .binary_search(list.last().unwrap())
///                 .unwrap_or_else(|p| p);
///             state.0.splice(position..position, list);
///             if state.0.len() == n {
///                 completed.store(true, Ordering::SeqCst);
///             }
///         });
///         return;
///     }
/// 
///     let mut sample_list: Vec<_> = list
///         .choose_multiple(&mut thread_rng(), p - 1)
///         .cloned()
///         .collect();
///     sample_list.sort_unstable();
///     sample_list.push(u32::MAX);
///     for (low, high) in once(&u32::MIN).chain(&sample_list).zip(&sample_list) {
///         let list = list
///             .iter()
///             .filter(|n| (low..high).contains(n))
///             .cloned()
///             .collect();
///         let completed = completed.clone();
///         submit.stateless(move |shared| sort_internal(&shared.submit, list, p, n, completed));
///     }
/// }
/// 
/// fn main() {
///     let list: Vec<_> = thread_rng()
///         .sample_iter(Uniform::new(u32::MIN, u32::MAX))
///         .take(1000)
///         .collect();
///     let mut expected = list.clone();
///     expected.sort_unstable();
///     let state = Handle::from(SortingState(Vec::new()));
///     let completed = Arc::new(AtomicBool::new(false));
///     // can be either `with_stateless` or `with_stateful`, we only need to
///     // access injected `submit` property
///     state.with_stateless({
///         let completed = completed.clone();
///         move |shared| sort(&shared.submit, list, 2, completed)
///     });
///     // donate current thread to stage for executing tasks
///     // you can donate more thread to speed it up
///     state.run_worker(move || completed.load(Ordering::SeqCst));
///     state.with_stateful(|state| assert_eq!(state.0, expected));
/// }
/// ```
#[cfg(not(test))]
pub mod stage;
#[cfg(test)]
#[path = "stage.rs"]
pub mod stage_prod; // for production
#[cfg(test)]
pub mod stage {
    pub use crate::simulated::{Handle, StatefulContext, StatelessContext, Submit};
    pub use crate::stage_prod::State;
}

/// Common definitions. Extract them so future refactor can be easier.
///
/// # Difference between `common` and `framework`
///
/// The common module is more specification-like, while [`framework`] module
/// contains more implementation. For example, we keep
/// [`ReplicaId`](common::ReplicaId) alias in common module, so if one day we
/// decide to change its aliasing target (again actually, originally it was
/// `u8`), we only need to change it once in common module instead of finding
/// every occurance in the codebase. Framework module does not serve this
/// propose.
///
/// In practice, common module can be depended by anything, even from [`facade`]
/// and [`protocol`], while framework should not be depended by either of them.
pub mod common;

/// Protocol implementations.
///
/// This module exports implementations of [`Receiver`](facade::Receiver) and
/// [`Invoke`](facade::Invoke).
///
/// # Implementation convension
///
/// `Receiver`s provide a `register_new(config, &mut transport, ...)` function,
/// which constructs a receiver instance and register it to `transport`. This
/// mimics the behavior of specpaxos `TransportReceiver` constructor. However,
/// the conventional argument order is different: besides `config` and
/// `transport`, for server side receiver the third argument is usually
/// `replica_id`, and the forth is usually `app`. These arguments are commonly
/// required by almost all receivers.
///
/// The `Invoke`able receivers should not depend on specific asynchronous
/// facility. Instead, they should access to asynchronous functionality through
/// [`AsyncEcosystem`](facade::AsyncEcosystem) trait.
///
/// The non-`Invoke`able receivers, i.e. server nodes should be built upon
/// [`stage`], even for the single-threaded ones.
pub mod protocol {
    pub mod hotstuff;
    pub mod pbft;
    pub mod unreplicated;
}

/// Application implementations.
///
/// This module exports implementation of [`App`](facade::App). The simplest
/// application may be "client-free". They don't require complicated message
/// encoding or client-side behavior. One example of these applications is
/// timestamp server.
///
/// For other applications, e.g. client-coordinated transactional store, they
/// provide customized client along with the `App`. These clients has various
/// interfaces, but they probably leverage `Invoke`able, i.e. protocol client
/// to proceed communication.
pub mod app {
    pub mod mock;
}

/// Engineering components.
///
/// The propose here is to keep protocol implementations minimal. Decouple
/// protocol implementations with reality helps improve protocol module's
/// readibility. (As this implied, code in framework is generally harder to
/// follow.)
///
/// The framework components expose all kinds of interfaces. The essential ones
/// are [`Transport`](facade::Transport) and
/// [`AsyncEcosystem`](facade::AsyncEcosystem) implementations. The submodules
/// are named after invovled lower-level stuff, and the structure is flattened.
pub mod framework {
    pub mod busy_poll;
    pub mod dpdk;
    #[cfg(any(feature = "tokio", test))]
    pub mod tokio;

    /// Convenient library for latency measurement.
    ///
    /// This module is a thin wrapper around [quanta] and [hdrhistogram]. It
    /// provides a similar but more user-friendly interface to specpaxos's latency
    /// library, especially for multithreaded usage.
    pub mod latency;
}

#[cfg(test)]
pub mod tests {
    use lazy_static::lazy_static;

    lazy_static! {
        pub static ref TRACING: () = {
            tracing_subscriber::fmt::init();
            // panic_abort();
        };
    }
}
