use std::{
    error::Error,
    fmt::{self, Display, Formatter},
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

use bincode::Options;
use k256::ecdsa::{
    signature::{Signer, Verifier},
    Signature,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_derive::{Deserialize, Serialize};

// should we re-type sha2 as well?
pub type SigningKey = k256::ecdsa::SigningKey;
pub type VerifyingKey = k256::ecdsa::VerifyingKey;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedMessage<M> {
    inner: Vec<u8>,
    // it seems like `Signature` has a buggy serde implementation, which cannot
    // work well with bincode's `deserialize_from`
    // signature: Signature, // do I want to generalize signing algorithm?
    signature: ([u8; 32], [u8; 32]), // serde not support [u8; 64] yet
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

#[derive(Debug, Clone)]
pub struct VerifiedMessage<M>(M, SignedMessage<M>);

impl<M> Deref for VerifiedMessage<M> {
    type Target = M;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<M> DerefMut for VerifiedMessage<M> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<M> SignedMessage<M> {
    pub fn sign(message: M, key: &SigningKey) -> Self
    where
        M: Serialize,
    {
        let inner = bincode::DefaultOptions::new().serialize(&message).unwrap();
        let signature: Signature = key.sign(&inner);
        let signature = signature.to_vec();
        Self {
            inner,
            signature: (
                signature[..32].try_into().unwrap(),
                signature[32..].try_into().unwrap(),
            ),
            _marker: PhantomData,
        }
    }

    pub fn verify(self, key: &VerifyingKey) -> Result<VerifiedMessage<M>, InauthenticMessage>
    where
        M: DeserializeOwned,
    {
        let (sig_a, sig_b) = self.signature;
        let signature: Signature = if let Ok(signature) = [sig_a, sig_b].concat()[..].try_into() {
            signature
        } else {
            return Err(InauthenticMessage);
        };
        if key.verify(&self.inner, &signature).is_ok() {
            if let Ok(message) = bincode::DefaultOptions::new().deserialize(&self.inner) {
                return Ok(VerifiedMessage(message, self));
            }
        }
        Err(InauthenticMessage)
    }

    // upgrade to VerifiedMessage as well?
    pub fn assume_verified(self) -> M
    where
        M: DeserializeOwned,
    {
        bincode::DefaultOptions::new()
            .deserialize(&self.inner)
            .unwrap()
    }
}

impl<M> VerifiedMessage<M> {
    pub fn signed_message(&self) -> &SignedMessage<M> {
        &self.1
    }
}
