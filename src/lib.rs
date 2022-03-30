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
/// For practical usage consider [`framework::dpdk::Transport`].
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
/// # Example
///
/// Work in progress.
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
/// `Receiver`s provide a `register_new(&mut transport, ...)` function, which
/// constructs a receiver instance and register it to `transport`. This mimics
/// the behavior of specpaxos `TransportReceiver` constructor.
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
