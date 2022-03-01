use std::io::Cursor;

use bincode::serialize_into;
use rand::{distributions::Alphanumeric, thread_rng, Rng};
use serde::Serialize;

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

pub fn serialize<M: Serialize>(message: M) -> impl FnOnce(&mut [u8]) -> u16 {
    move |buffer| {
        let mut cursor = Cursor::new(buffer);
        serialize_into(&mut cursor, &message).unwrap();
        cursor.position() as u16
    }
}
