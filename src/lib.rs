pub mod common;
pub mod model;

#[cfg(test)]
pub(crate) mod simulated;
pub mod dpdk;

pub mod replication {
    pub mod unreplicated;
}

pub mod app {
    #[cfg(test)]
    pub(crate) mod mock;
}
