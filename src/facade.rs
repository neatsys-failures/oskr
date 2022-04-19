use std::{
    convert::Infallible, fs::File, io::Read, ops::Range, path::Path, str::FromStr, time::Instant,
};

use async_trait::async_trait;
use futures::Future;

use crate::common::{OpNumber, Opaque, SigningKey};

/// Asynchronized invoking interface.
///
/// Currently the trait is polyfilled with [mod@async_trait], which makes the
/// signature in document looks weird.
///
/// A minimal example to implement `Invoke` that never completes:
///
/// ```
/// # use async_trait::async_trait;
/// # use futures::future::pending;
/// # use oskr::{common::Opaque, facade::Invoke};
/// struct NeverComplete;
/// #[async_trait]
/// impl Invoke for NeverComplete {
///     async fn invoke(&mut self, op: Opaque) -> Opaque {
///         pending().await
///     }
/// }
/// ```
#[async_trait]
pub trait Invoke {
    async fn invoke(&mut self, op: Opaque) -> Opaque;
}

/// State machine application which may support speculative execution and
/// rollback.
///
/// The leader upcall in specpaxos in removed and not supported, because this
/// is a trait for more general application model, not only replicated ones.
///
/// For applications that support rollback: implement `execute` with speculative
/// execution. The `op_number` is subject to be `rollback`ed, until it is
/// `commit`ted later.
///
/// For applications that not support rollback: implement `execute` as
/// irreversible operation, and use default implementation of `rollback`, which
/// correctly prevent protocol calling it.
#[allow(unused_variables)]
pub trait App {
    /// Protocol promise `op_number` is strictly ascending through calls when
    /// there is no rollback. However, there could be gap.
    ///
    /// It is unspecified whether protocol allows to reuse op number after
    /// rollback, but I think they do not.
    fn execute(&mut self, op_number: OpNumber, op: Opaque) -> Opaque;
    fn rollback(
        &mut self,
        current: OpNumber,
        to: OpNumber,
        op_list: &mut dyn Iterator<Item = (OpNumber, Opaque)>,
    ) {
        unimplemented!()
    }
    fn commit(&mut self, op_number: OpNumber) {}
}

/// Abstraction for async ecosystem.
///
/// The concept of async ecosystem comes from [the async book][1]. It refers to
/// a bundle of async executor and reactors.
///
/// [1]: https://rust-lang.github.io/async-book/08_ecosystem/00_chapter.html
///
/// The asynchronized receiver in this codebase usually requires timers,
/// concurrent tasks, etc. As the result they cannot be ecosystem dependent. The
/// `AsyncEcosystem` trait abstract specific ecosystem implementations to make
/// the receivers work with any ecosystem, as long as there is an implementation
/// of `AsyncEcosystem` for it.
///
/// **About reusing standard abstraction.** Currently I cannot found any
/// community attemption on defining a standard async ecosystem abstraction. The
/// author of async-std stands against the idea, which may imply the motivation
/// cannot be satisfied. It also seems like several projects, which also do not
/// prefer conditional compiling solution, are building their own abstractions
/// as well, so I probably has to do the same, sadly.
///
/// The trait is kept minimal, and only essential interfaces are picked. Notice
/// that interfaces are free-function style, which may only be valid in certain
/// context. It is user's responsiblity to provide proper context, e.g. set up
/// a tokio runtime and create receivers that depend on
/// [`framework::tokio::AsyncEcosystem`](crate::framework::tokio::AsyncEcosystem)
/// in it, or the receiver will probably fail at runtime.

// wait for GAT to remove trait generic parameter
pub trait AsyncEcosystem<T> {
    /// Handle for a spawned task, can be use to either await on or cancel.
    ///
    /// The lack of interface to detach a task is intentional. An
    /// `AsyncEcosystem` implementation is not required to be able to do detach,
    /// and it is encouraged to explicitly hold task handle until it is
    /// discontinued.
    ///
    /// The dropping behavior of a handle is not specified. Do not drop any
    /// handle out of `AsyncEcosystem`. Instead, either await on it or consume
    /// it by calling `cancel`.
    type JoinHandle: Future<Output = T> + Send;
    type Sleep: Future<Output = ()> + Send;

    fn spawn(task: impl Future<Output = T> + Send + 'static) -> Self::JoinHandle;
    fn cancel(handle: Self::JoinHandle);
    fn sleep_until(instant: Instant) -> Self::Sleep;
}

pub trait Transport
where
    Self: 'static,
{
    type Address: Clone + Eq + Send + Sync;
    type RxBuffer: AsRef<[u8]> + Send;
    // TxAgent has to be Sync for now, because it may show up in stage's shared
    // state and be accessed concurrently from multiple threads
    // this may be bad for two reason: it was designed as Send + !Sync in mind
    // from beginning; if in the future we want to reimplement dpdk transport,
    // which promise there is at most one TxAgent instance per thread, then it
    // may cache worker id and become !Sync for real
    // (or maybe just put it into thread local data?)
    type TxAgent: TxAgent<Transport = Self> + Clone + Send + Sync;

    fn tx_agent(&self) -> Self::TxAgent;

    fn register(
        &mut self,
        receiver: &impl Receiver<Self>,
        rx_agent: impl Fn(Self::Address, Self::RxBuffer) + 'static + Send,
    ) where
        Self: Sized;
    fn register_multicast(
        &mut self,
        rx_agent: impl Fn(Self::Address, Self::RxBuffer) + 'static + Send,
    );

    fn ephemeral_address(&self) -> Self::Address;
    fn null_address() -> Self::Address;
}

pub trait Receiver<T: Transport> {
    fn get_address(&self) -> &T::Address;
    // anything else?
}

pub trait TxAgent {
    type Transport: Transport;

    fn send_message(
        &self,
        source: &impl Receiver<Self::Transport>,
        dest: &<Self::Transport as Transport>::Address,
        message: impl FnOnce(&mut [u8]) -> u16,
    );

    /// This cannot be replaced by a loop calling to `send_message`, because
    /// `message` closure only allow to be invoked once.
    fn send_message_to_all(
        &self,
        source: &impl Receiver<Self::Transport>,
        dest_list: &[<Self::Transport as Transport>::Address],
        message: impl FnOnce(&mut [u8]) -> u16,
    );
}

/// Network configuration.
///
/// This struct is precisely mapped from configuration file content. The
/// [`Config`](crate::common::Config) in common module wraps it into the
/// interface suitable for various protocols.
// consider move to dedicated module if getting too long
pub struct Config<T: Transport + ?Sized> {
    pub replica: Vec<T::Address>,
    pub group: Vec<Range<usize>>,
    pub multicast: Option<T::Address>,
    pub f: usize,
    // for non-signed protocol this is empty
    pub signing_key: Vec<(T::Address, SigningKey)>,
}

impl<T: Transport + ?Sized> Clone for Config<T> {
    fn clone(&self) -> Self {
        Self {
            replica: self.replica.clone(),
            group: self.group.clone(),
            multicast: self.multicast.clone(),
            f: self.f,
            signing_key: self.signing_key.clone(),
        }
    }
}

impl<T: Transport + ?Sized> FromStr for Config<T>
where
    T::Address: FromStr,
{
    type Err = Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut replica_address = Vec::new();
        let mut group = Vec::new();
        let mut group_start = None;
        let mut multicast_address = None;
        let mut n_fault = None;
        for line in s.lines() {
            let line = if let Some((line, _)) = line.split_once('#') {
                line.trim()
            } else {
                line.trim()
            };
            if line.is_empty() {
                continue;
            }
            let (prompt, value) = line.split_once(char::is_whitespace).unwrap();
            let error_message = "failed to parse replica address";
            match prompt {
                "f" => n_fault = Some(value.parse().unwrap()),
                "replica" => {
                    replica_address.push(value.parse().map_err(|_| error_message).unwrap())
                }
                "group" => {
                    if !replica_address.is_empty() {
                        group.push(group_start.unwrap()..replica_address.len());
                    }
                    group_start = Some(replica_address.len());
                }
                "multicast" => {
                    multicast_address = Some(value.parse().map_err(|_| error_message).unwrap())
                }
                _ => panic!("unexpect prompt: {}", prompt),
            }
        }
        if let Some(group_start) = group_start {
            group.push(group_start..replica_address.len());
        }
        Ok(Self {
            replica: replica_address,
            group,
            multicast: multicast_address,
            f: n_fault.unwrap(),
            signing_key: Vec::new(), // fill later
        })
    }
}

impl<T: Transport + ?Sized> Config<T> {
    pub fn collect_signing_key(&mut self, path: &Path) {
        for (i, replica) in self.replica.iter().enumerate() {
            let prefix = path.file_name().unwrap().to_str().unwrap();
            let key = path.with_file_name(format!("{}-{}.pem", prefix, i));
            let key = {
                let mut buf = String::new();
                File::open(key).unwrap().read_to_string(&mut buf).unwrap();
                buf
            };
            let key = SigningKey::K256(key.parse().unwrap());
            self.signing_key.push((replica.clone(), key));
        }
    }
}
