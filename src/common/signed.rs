//! Common interface for signing and verification.
//!
#![doc = include_str!("signed.svg")]
//!
//! The module serves for two proposes:
//! * Hide crypto related dependences behind it, to make it easier to replace
//!   when necessary.
//! * Provide a simple and useful abstraction for signed message, make efforts
//!   to encourage protocols not to keep bare signatures.
//!
//! The basic relation and transformation among message types are shown in the
//! diagram. Additionally:
//! * `SignedMessage` contains only serialized data, and should be used in
//!   messages for transferring and bookkeeping.
//! * `VerifiedMessage` bridges between the other two, and receiving side can
//!   perform most processing against it.
//! * The clear message should be constructed by sending side for signing.
//!
//! # Choose Rust Crypto or libsecp256k1
//!
//! Work in progress.

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
use secp256k1::{Message, Secp256k1};
use serde::{de::DeserializeOwned, Serialize};
use serde_derive::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

#[derive(Debug, Clone)]
pub enum SigningKey {
    K256(k256::ecdsa::SigningKey),
    Secp256k1(secp256k1::SecretKey),
}

#[derive(Debug, Clone)]
pub enum VerifyingKey {
    K256(k256::ecdsa::VerifyingKey),
    Secp256k1(secp256k1::PublicKey),
}

impl SigningKey {
    pub fn verifying_key(&self) -> VerifyingKey {
        let secp = Secp256k1::new();
        match self {
            Self::K256(key) => VerifyingKey::K256(key.verifying_key()),
            Self::Secp256k1(key) => {
                VerifyingKey::Secp256k1(secp256k1::PublicKey::from_secret_key(&secp, key))
            }
        }
    }

    pub fn use_secp256k1(self) -> Self {
        if let Self::K256(key) = self {
            Self::Secp256k1(secp256k1::SecretKey::from_slice(&*key.to_bytes()).unwrap())
        } else {
            unreachable!()
        }
    }
}

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

thread_local! {
    pub static SECP: Secp256k1<secp256k1::All> = Secp256k1::new();
}
impl<M> SignedMessage<M> {
    pub fn from_data(inner: Vec<u8>, signature: [u8; 64]) -> Self {
        Self {
            inner,
            signature: (
                signature[..32].try_into().unwrap(),
                signature[32..].try_into().unwrap(),
            ),
            _marker: PhantomData,
        }
    }

    pub fn sign(message: M, key: &SigningKey) -> Self
    where
        M: Serialize,
    {
        let inner = bincode::DefaultOptions::new().serialize(&message).unwrap();
        match key {
            SigningKey::K256(key) => {
                let signature: Signature = key.sign(&inner);
                let signature = signature.to_vec();
                Self::from_data(inner, signature.try_into().unwrap())
            }
            SigningKey::Secp256k1(key) => {
                let message = Message::from_slice(&*Sha256::digest(&*inner)).unwrap();
                let signature =
                    SECP.with(|secp| secp.sign_ecdsa(&message, key).serialize_compact());
                Self::from_data(inner, signature)
            }
        }
    }

    pub fn verify(self, key: &VerifyingKey) -> Result<VerifiedMessage<M>, InauthenticMessage>
    where
        M: DeserializeOwned,
    {
        let (sig_a, sig_b) = self.signature;
        let verified = match key {
            VerifyingKey::K256(key) => {
                let signature: Signature =
                    if let Ok(signature) = [sig_a, sig_b].concat()[..].try_into() {
                        signature
                    } else {
                        return Err(InauthenticMessage);
                    };
                key.verify(&self.inner, &signature).is_ok()
            }
            VerifyingKey::Secp256k1(key) => {
                let signature = if let Ok(signature) =
                    secp256k1::ecdsa::Signature::from_compact(&[sig_a, sig_b].concat())
                {
                    signature
                } else {
                    return Err(InauthenticMessage);
                };
                let message = Message::from_slice(&*Sha256::digest(&*self.inner)).unwrap();
                SECP.with(|secp| secp.verify_ecdsa(&message, &signature, key).is_ok())
            }
        };
        if verified {
            if let Ok(message) = bincode::DefaultOptions::new().deserialize(&self.inner) {
                return Ok(VerifiedMessage(message, self));
            }
        }
        Err(InauthenticMessage)
    }

    // upgrade to VerifiedMessage as well?
    pub fn assume_verified(&self) -> M
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
