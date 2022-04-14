use std::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

use serde::{de::DeserializeOwned, Serialize};

use crate::common::{deserialize, serialize, signed::InauthenticMessage};

// this struct is not designed to be transfered, so it is not (de)serailizable
// but it is still suitable for bookkeeping
#[derive(Debug, Clone)]
pub struct TrustedOrderedMulticast<M> {
    // in order to keep the format absolutely stable to be exposed to hardware
    // we are using untyped array as representation, instead of C-style struct
    // as layout
    buffer: [u8; 101], // cannot use OFFSET_END?
    message: Vec<u8>,
    _m: PhantomData<M>,
}

pub struct VerifiedOrderedMulticast<M> {
    pub trusted: TrustedOrderedMulticast<M>,
    pub sequence_number: u32,
    pub session_number: u8,
    message: M,
}

impl<M> TrustedOrderedMulticast<M> {
    const OFFSET_DIGEST: usize = 0;
    const OFFSET_SEQUENCE: usize = 32;
    const OFFSET_SESSION: usize = 36;
    const OFFSET_SIGNATURE: usize = 37;
    const OFFSET_END: usize = 101;

    pub fn send(message: M, buffer: &mut [u8]) -> u16
    where
        M: Serialize,
    {
        buffer[..Self::OFFSET_END].fill(0);
        Self::OFFSET_END as u16 + serialize(message)(&mut buffer[Self::OFFSET_END..])
    }

    pub fn new(buffer: &[u8]) -> Self {
        Self {
            buffer: buffer[..Self::OFFSET_END].try_into().unwrap(),
            message: buffer[Self::OFFSET_END..].to_vec(),
            _m: PhantomData,
        }
    }

    pub fn verify(
        self,
        verifying_key: (),
    ) -> Result<VerifiedOrderedMulticast<M>, InauthenticMessage>
    where
        M: DeserializeOwned,
    {
        // TODO verify

        let message = if let Ok(message) = deserialize(&*self.message) {
            message
        } else {
            return Err(InauthenticMessage);
        };
        Ok(VerifiedOrderedMulticast {
            sequence_number: u32::from_be_bytes(
                self.buffer[Self::OFFSET_SEQUENCE..Self::OFFSET_SEQUENCE + 4]
                    .try_into()
                    .unwrap(),
            ),
            session_number: self.buffer[Self::OFFSET_SESSION],
            message,
            trusted: self,
        })
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
