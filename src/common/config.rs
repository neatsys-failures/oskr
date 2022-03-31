//! Configuration adapter.
//!
//! In specpaxos configuration contains all settings collected at runtime,
//! including server addresses, number of tolerance, etc. User can change these
//! settings between runs without recompiling. User has to make sure every
//! node read the same configuration, or the system will not work.
//!
//! The [`Config`](crate::facade::Config) in facade is almost same to the one in
//! specpaxos and its derivatives. It parses file content and holds identical
//! information as parsed files, no more no less. Transport and receiver
//! implementation can read according to their wish.
//!
//! The model become problematic in [Eris], where configuration contains
//! multiple groups, and transactional protocols are group-aware. The required
//! group-related functionality has to be provided by transport layer, e.g.
//! methods like `send_message_to_group`, making transport layer keeps too many
//! interfaces. It also makes code hard to maintainance to keep backward
//! compability to those group-unaware protocols.
//!
//! [Eris]: https://github.com/UWSysLab/Eris
//!
//! In Oskr it becomes even trickier: we need to serve secret keys for
//! protocols. Typically an identity provider that required by BFT protocols
//! should be centralized and dynamically updated, but for benchmark propose
//! a fixed identity configuration should be sufficient.
//!
//! To overcome above problems, we abstract a configuration adapter in common
//! module. It is based on the file-based configuration facade, and is designed
//! to be depended by protocols (both group-aware and -unaware) and transport
//! implementations.
//!
//! # Configuration mode
//!
//! We define the configuration file which has no "group" lines as in *minimal
//! mode*, and define grouped configuration file as in *standard mode*. Base on
//! configuration file mode, we have three different kinds of adapter:
//! * Classical adapter reads minimal configuration and provide group-unaware
//!   interfaces.
//! * Shard adapter reads standard configuration and provide group-unaware
//!   interfaces by trimming specific group from configuration.
//!
//!   For example, consider a standard configuration with s1, s2 in group 0, and
//!   s3, s4 in group 1. A shard adapter for group 0 will behave as there are
//!   only two servers s1, s2 in the system, so a `send_message_to_all` call
//!   will only send to these two servers.
//! * Global adapter reads standard configuration and provide group-aware
//!   interfaces. Group-aware protocols can access to all remote groups from a
//!   global adapter.
//!
//! # Identity service
//!
//! For the sake of simplicity, we identify nodes based on address, and assume
//! node's address never changes. Only well-known addresses, i.e. addresses
//! present in configuration file, have identities. Client receivers who use
//! ephemeral addresses cannot sign their messages, but they still can verify
//! messages received from servers.
//!
//! If configuration files do not include secret keys, i.e. in a non-security
//! setup, then no one has identity at all.

use std::ops::{Deref, RangeFull};

use crate::{
    common::{ReplicaId, SigningKey, VerifyingKey, ViewNumber},
    facade::{self, Receiver, Transport},
};

pub struct Config<T: Transport> {
    inner: ConfigInner<T>,
    verifying_key: Vec<(T::Address, VerifyingKey)>,
}

enum ConfigInner<T: Transport> {
    Shard(facade::Config<T>, usize),
    Global(facade::Config<T>),
}

impl<T: Transport> Clone for Config<T> {
    fn clone(&self) -> Self {
        Self {
            inner: match &self.inner {
                ConfigInner::Shard(config, group_id) => {
                    ConfigInner::Shard(config.clone(), *group_id)
                }
                ConfigInner::Global(config) => ConfigInner::Global(config.clone()),
            },
            verifying_key: self.verifying_key.clone(),
        }
    }
}

impl<T: Transport> Config<T> {
    pub fn for_shard(config: facade::Config<T>, group_id: usize) -> Self {
        if config.group.is_empty() {
            // classical
            assert_eq!(group_id, 0);
        }
        Self {
            verifying_key: config
                .signing_key
                .iter()
                .map(|(address, key)| (address.clone(), key.verifying_key()))
                .collect(),
            inner: ConfigInner::Shard(config, group_id),
        }
    }

    pub fn for_global(config: facade::Config<T>) -> Self {
        Self {
            verifying_key: config
                .signing_key
                .iter()
                .map(|(address, key)| (address.clone(), key.verifying_key()))
                .collect(),
            inner: ConfigInner::Global(config),
        }
    }

    pub fn signing_key(&self, receiver: &impl Receiver<T>) -> &SigningKey {
        &match &self.inner {
            ConfigInner::Shard(config, _) => config,
            ConfigInner::Global(config) => config,
        }
        .signing_key
        .iter()
        .find(|(address, _)| address == receiver.get_address())
        .unwrap()
        .1
    }

    pub fn verifying_key(&self, remote: &T::Address) -> Option<&VerifyingKey> {
        self.verifying_key
            .iter()
            .find(|(address, _)| address == remote)
            .map(|(_, key)| key)
    }

    pub fn replica<I: ReplicaIndex<T>>(&self, at: I) -> &I::Output {
        I::index(
            match &self.inner {
                ConfigInner::Shard(config, group_id) => {
                    &config.replica[config
                        .group
                        .get(*group_id)
                        .cloned()
                        .unwrap_or(0..config.replica.len())]
                }
                ConfigInner::Global(config) => &*config.replica,
            },
            at,
        )
    }

    pub fn view_primary(&self, view_number: ViewNumber) -> ReplicaId {
        (view_number as usize % self.replica(..).len()) as _
    }
}

impl<T: Transport> Deref for Config<T> {
    type Target = facade::Config<T>;
    fn deref(&self) -> &Self::Target {
        match &self.inner {
            ConfigInner::Shard(config, _) => config,
            ConfigInner::Global(config) => config,
        }
    }
}

pub trait ReplicaIndex<T: Transport> {
    type Output: ?Sized;
    fn index(replica: &[T::Address], at: Self) -> &Self::Output;
}

impl<T: Transport> ReplicaIndex<T> for ReplicaId {
    type Output = T::Address;
    fn index(replica: &[T::Address], at: Self) -> &Self::Output {
        &replica[at as usize]
    }
}

impl<T: Transport> ReplicaIndex<T> for RangeFull {
    type Output = [T::Address];
    fn index(replica: &[T::Address], _at: Self) -> &Self::Output {
        replica
    }
}
