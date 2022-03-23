pub mod facade;

pub mod dpdk_shim;

#[cfg(any(test, doc))]
pub mod simulated;

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

pub mod common;

pub mod protocol {
    pub mod pbft;
    pub mod unreplicated;
}

pub mod app {
    pub mod mock;
}

pub mod latency;

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
