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
/// For practical usage consider [`runtime::dpdk::Transport`].
pub mod dpdk_shim;

/// Simulated facilities for writing test cases.
#[cfg(any(test, doc))]
pub mod simulated;

/// Stage abstraction. Receiver on stage can use multiple threads efficiently.
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

/// Convenient library for latency measurement.
///
/// This module is a thin wrapper around [quanta] and [hdrhistogram]. It
/// provides a similar but more user-friendly interface to specpaxos's latency
/// library, especially for multithreaded usage.
pub mod latency;

/// Various runtime facilities that supports protocol implementations.
///
/// This module exports implementations of `Transport` and `AsyncEcosystem`.
/// The submodules are named after implementation base.
pub mod runtime {
    pub mod busy_poll;
    pub mod dpdk;
    #[cfg(any(feature = "tokio", test))]
    pub mod tokio;
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
