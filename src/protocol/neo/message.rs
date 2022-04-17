use std::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

use serde::{de::DeserializeOwned, Serialize};
use serde_derive::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::common::{deserialize, serialize, signed::InauthenticMessage, Digest};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderedMulticast<M> {
    pub chain_hash: Digest,
    pub sequence_number: u32,
    pub session_number: u8,
    signature0: [u8; 32],
    signature1: [u8; 32],
    message: Vec<u8>,
    _m: PhantomData<M>,
}

pub struct VerifiedOrderedMulticast<M> {
    pub meta: OrderedMulticast<M>,
    message: M,
}

impl<M> OrderedMulticast<M> {
    const OFFSET_DIGEST: usize = 0;
    const OFFSET_SEQUENCE: usize = 32;
    const OFFSET_SESSION: usize = 36;
    const OFFSET_SIGNATURE: usize = 37;
    const OFFSET_END: usize = 101;

    // the (de)ser interface pair for working with hardware
    // use bincode when transfer between nodes

    pub fn send(message: M, buffer: &mut [u8]) -> u16
    where
        M: Serialize,
    {
        buffer[Self::OFFSET_SEQUENCE..Self::OFFSET_END].fill(0);
        let message_length = serialize(message)(&mut buffer[Self::OFFSET_END..]);
        let mut digest: [_; 32] =
            Sha256::digest(&buffer[Self::OFFSET_END..Self::OFFSET_END + message_length as usize])
                .into();
        digest[28..].fill(0); // required by switch p4 program
        buffer[Self::OFFSET_DIGEST..Self::OFFSET_DIGEST + 32].clone_from_slice(&digest);
        message_length + Self::OFFSET_END as u16
    }

    pub fn parse(buffer: &[u8]) -> Self {
        Self {
            chain_hash: buffer[Self::OFFSET_DIGEST..Self::OFFSET_DIGEST + 32]
                .try_into()
                .unwrap(),
            sequence_number: u32::from_be_bytes(
                buffer[Self::OFFSET_SEQUENCE..Self::OFFSET_SEQUENCE + 4]
                    .try_into()
                    .unwrap(),
            ),
            session_number: buffer[Self::OFFSET_SESSION],
            signature0: buffer[Self::OFFSET_SIGNATURE..Self::OFFSET_SIGNATURE + 32]
                .try_into()
                .unwrap(),
            signature1: buffer[Self::OFFSET_SIGNATURE + 32..Self::OFFSET_SIGNATURE + 64]
                .try_into()
                .unwrap(),
            message: buffer[Self::OFFSET_END..].to_vec(),
            _m: PhantomData,
        }
    }

    pub fn is_signed(&self) -> bool {
        self.signature0 != [0; 32] || self.signature1 != [0; 32]
    }

    pub fn verify(
        self,
        verifying_key: (),
    ) -> Result<VerifiedOrderedMulticast<M>, InauthenticMessage>
    where
        M: DeserializeOwned,
    {
        if self.is_signed() {
            // verify
        }
        return deserialize(&*self.message)
            .map(|message| VerifiedOrderedMulticast {
                message,
                meta: self,
            })
            .map_err(|_| InauthenticMessage);
    }
}

impl<M> Deref for VerifiedOrderedMulticast<M> {
    type Target = M;
    fn deref(&self) -> &Self::Target {
        &self.message
    }
}

impl<M> DerefMut for VerifiedOrderedMulticast<M> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.message
    }
}
