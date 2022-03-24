use serde_derive::{Deserialize, Serialize};

use crate::common::{
    signed::InauthenticMessage, ClientId, Digest, OpNumber, Opaque, ReplicaId, RequestNumber,
    SignedMessage, VerifyingKey,
};

// HotStuff paper omit much implementation details, maybe too much.
// some noticeable specification/modification by this implementation:
// * Separate Msg(Generic, ...) and VoteMsg(Generic, ...) types. By the way,
//   remove message type field from signed content.
// * Because vote message now only contains (view number, node, signature of
//   (view number, node)), it is represented as SignedMessage<(view number,
//   node)> directly.
// * Generic message carry full node, and the `node` in VoteGeneric and QC is
//   represented as node's digest, because we assume the receiver probably get
//   the node content already.
// * Add replica id field to VoteGeneric so we can count the number of
//   deduplicated votes. (I don't want to deduplicate base on remote address.)
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
// * The view number field is removed, because according to the paper, event-
//   driven HotStuff does not even store current view number.

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
    pub node: Option<GenericNode>,
    pub justify: Option<QuorumCertification>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoteGeneric {
    pub node: Digest,
    pub replica_id: ReplicaId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reply {
    pub request_number: RequestNumber,
    pub result: Opaque,
    pub replica_id: ReplicaId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenericNode {
    pub parent: Digest,
    pub command: Vec<Request>,
    pub justify: QuorumCertification,
    pub height: OpNumber,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QuorumCertification(Vec<(ReplicaId, SignedMessage<VoteGeneric>)>);

impl QuorumCertification {
    pub fn verify<'a>(
        &'a self,
        // will be replaced with one single public key when threshold signature
        // is deployed
        verifying_key: impl Fn(ReplicaId) -> &'a VerifyingKey,
        threshold: usize,
    ) -> Result<Digest, InauthenticMessage> {
        assert!(threshold > 0);
        if self.0.len() < threshold {
            return Err(InauthenticMessage);
        }

        let (replica0, vote0) = self.0[0].clone();
        let node = if let Ok(verified) = vote0.verify(verifying_key(replica0)) {
            verified.node
        } else {
            return Err(InauthenticMessage);
        };

        for (replica, vote) in self.0[1..].iter().cloned() {
            if let Ok(verified) = vote.verify(verifying_key(replica)) {
                if verified.node != node {
                    return Err(InauthenticMessage); // more strict than necessary, but simpler
                }
            } else {
                return Err(InauthenticMessage);
            }
        }
        Ok(node)
    }
}
