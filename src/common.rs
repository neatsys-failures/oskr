use std::{
    error::Error,
    fmt::{self, Display, Formatter},
    io::{Cursor, Read, Write},
    panic::{set_hook, take_hook},
    process,
};

use bincode::Options;
use rand::{distributions::Alphanumeric, thread_rng, Rng};
use serde::{Deserialize, Serialize};

pub mod signed;
pub use signed::SignedMessage;

pub type ReplicaId = u8;
pub type ClientId = [u8; 4];

pub fn generate_id() -> ClientId {
    let mut rng = thread_rng();
    let mut id = [0; 4];
    for byte in &mut id {
        *byte = rng.sample(Alphanumeric);
    }
    id
}

pub type RequestNumber = u32;
pub type ViewNumber = u8;
pub type OpNumber = u32;
pub type Opaque = Vec<u8>;
pub type Digest = [u8; 32];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MalformedMessage;
impl Display for MalformedMessage {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "malformed message")
    }
}
impl Error for MalformedMessage {}

// providing deserialize to avoid accidentially using bincode::deserialize
// not unwrap by default for the sake of byzantine network
pub fn deserialize<M: for<'a> Deserialize<'a>>(reader: impl Read) -> Result<M, MalformedMessage> {
    bincode::DefaultOptions::new()
        .allow_trailing_bytes()
        .deserialize_from(reader)
        .map_err(|_| MalformedMessage)
}

pub fn serialize<M: Serialize, T: ?Sized>(message: M) -> impl FnOnce(&mut T) -> u16
where
    for<'a> Cursor<&'a mut T>: Write,
{
    move |buffer| {
        let mut cursor = Cursor::new(buffer);
        bincode::DefaultOptions::new()
            .serialize_into(&mut cursor, &message)
            .unwrap();
        cursor.position() as u16
    }
}

// consider move to util
pub fn panic_abort() {
    let default_hook = take_hook();
    set_hook(Box::new(move |info| {
        default_hook(info);
        println!("[oskr] force abort");
        process::abort();
    }));
}
