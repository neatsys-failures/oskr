use std::{
    error::Error,
    fmt::{self, Display, Formatter},
    marker::PhantomData,
};

use bincode::Options;
use k256::ecdsa::{
    signature::{Signer, Verifier},
    Signature, SigningKey, VerifyingKey,
};
use serde::{Deserialize, Serialize};
use serde_derive::{Deserialize, Serialize};

use crate::common::MalformedMessage;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedMessage<M> {
    inner: Vec<u8>,
    signature: Signature, // do I want to generalize signing algorithm?
    _marker: PhantomData<M>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InauthenticMessage;

impl Display for InauthenticMessage {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "inauthentic message")
    }
}
impl Error for InauthenticMessage {}

impl<M> SignedMessage<M> {
    pub fn sign(message: M, key: &SigningKey) -> Self
    where
        M: Serialize,
    {
        let inner = bincode::DefaultOptions::new().serialize(&message).unwrap();
        let signature = key.sign(&inner);
        Self {
            inner,
            signature,
            _marker: PhantomData,
        }
    }

    pub fn verify(self, key: &VerifyingKey) -> Result<M, Box<dyn Error + Send + Sync + 'static>>
    where
        M: for<'a> Deserialize<'a>,
    {
        if key.verify(&self.inner, &self.signature).is_ok() {
            if let Ok(message) = bincode::DefaultOptions::new().deserialize(&self.inner) {
                Ok(message)
            } else {
                Err(Box::new(MalformedMessage))
            }
        } else {
            Err(Box::new(InauthenticMessage))
        }
    }
}
