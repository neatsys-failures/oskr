use bincode::Options;
use lazy_static::lazy_static;
use serde_derive::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::common::{
    signed::InauthenticMessage, ClientId, Digest, OpNumber, Opaque, ReplicaId, RequestNumber,
    SignedMessage, VerifyingKey, ViewNumber,
};

// HotStuff paper omit much implementation details, maybe too much.
// some noticeable specification/modification by this implementation:
// * Disaggregate message types. Remove blank field of each message type.
// * Because vote message now only contains (view number, node, signature of
//   (view number, node)), it is represented as SignedMessage<(view number,
//   node)> directly.
// * Generic message carry full node, and the `node` in VoteGeneric and QC is
//   represented as node's digest, because we assume the receiver probably get
//   the node content already.
// * Add replica id field to VoteGeneric so we can count the number of
//   deduplicated votes.
//
//   In original HotStuff paper leader collect all votes with different (view
//   number, partial signature) pair, without checking voter. This is obviously
//   wrong, because one voter can vote in multiple views, and its votes will be
//   counted multiple times in the quorum.
//
//   Notice that in previous section `QC` is defined as a merger of partial
//   signatures who have the same view number, the paper has inconsistent
//   content and did not explain anywhere. Anyway, I choose to follow a
//   intuatively strict rule to implement: message view number should always
//   match current view number, or the message is ignored from ingress. This is
//   not necessary safe and also could break liveness, but it's the best I can
//   do.
// * Define QC as a vector of signed message and simulate threshold signature
//   by verifying them in sequence.
//
//   Rust community does not provide us many production-ready threshold
//   signature libraries which is well-known to be suitable here (neither C++
//   community does I think). I hope there will be some libraries in the future
//   so I don't need to implement by myself if necessary.
//
//   Additionally, libhotstuff do the simulation as well, so it is ok to
//   evaluate with this.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToReplica {
    Request(Request),
    // if I understand correctly Generic message is not signed in HotStuff paper
    // I think this fact does not break safety, although paper does not talk
    // about what will happen if anyone other than leader but also has a valid
    // QC proposes
    Generic(Generic),
    VoteGeneric(SignedMessage<VoteGeneric>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub op: Opaque,
    pub request_number: RequestNumber,
    pub client_id: ClientId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Generic {
    pub view_number: ViewNumber,
    pub node: GenericNode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoteGeneric {
    pub view_number: ViewNumber,
    pub node: Digest,
    pub replica_id: ReplicaId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reply {
    pub request_number: RequestNumber,
    pub result: Opaque,
    pub replica_id: ReplicaId,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GenericNode {
    pub parent: Digest,
    pub command: Vec<Request>,
    pub justify: QuorumCertification,
    pub height: OpNumber,
}

lazy_static! {
    pub static ref GENESIS: GenericNode = {
        let mut node = GenericNode::default();
        node.justify = QuorumCertification::default();
        node
    };
}

impl GenericNode {
    pub fn create_leaf(
        parent: &Digest,
        command: Vec<Request>,
        qc: QuorumCertification,
        height: OpNumber,
    ) -> Self {
        Self {
            parent: *parent,
            command,
            justify: qc,
            height,
        }
    }

    pub fn digest(&self) -> Digest {
        Sha256::digest(bincode::options().serialize(self).unwrap()).into()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QuorumCertification {
    pub view_number: ViewNumber,
    pub node: Digest,
    pub signature: Vec<(ReplicaId, SignedMessage<VoteGeneric>)>,
}

impl QuorumCertification {
    pub fn verify<'a>(
        &'a self,
        // will be replaced with one single public key when threshold signature
        // is deployed
        verifying_key: impl Fn(ReplicaId) -> &'a VerifyingKey,
        threshold: usize,
    ) -> Result<(), InauthenticMessage> {
        assert!(threshold > 0);
        if self.node == GENESIS.justify.node {
            return Ok(());
        }

        if self.signature.len() < threshold {
            return Err(InauthenticMessage);
        }

        for (replica, vote) in self.signature.iter().cloned() {
            if let Ok(verified) = vote.verify(verifying_key(replica)) {
                if verified.view_number != self.view_number || verified.node != self.node {
                    return Err(InauthenticMessage); // more strict than necessary, but simpler
                }
            } else {
                return Err(InauthenticMessage);
            }
        }
        Ok(())
    }
}
