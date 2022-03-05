use std::io::Cursor;

use bincode::Options;
use rand::{distributions::Alphanumeric, thread_rng, Rng};
use serde::{Deserialize, Serialize};

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
pub type OpNumber = u32;
pub type Opaque = Vec<u8>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MalformedMessage;
// providing deserialize to avoid accidentially using bincode::deserialize
// not unwrap by default for the sake of byzantine network
pub fn deserialize<M: for<'a> Deserialize<'a>>(bytes: &[u8]) -> Result<M, MalformedMessage> {
    bincode::DefaultOptions::new()
        .allow_trailing_bytes()
        .deserialize(bytes)
        .map_err(|_| MalformedMessage)
}

pub fn serialize<M: Serialize>(message: M) -> impl FnOnce(&mut [u8]) -> u16 {
    move |buffer| {
        let mut cursor = Cursor::new(buffer);
        bincode::DefaultOptions::new()
            .serialize_into(&mut cursor, &message)
            .unwrap();
        cursor.position() as u16
    }
}
