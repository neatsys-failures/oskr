use serde_derive::{Deserialize, Serialize};

use crate::common::{
    signed::SignedMessage, ClientId, Digest, OpNumber, Opaque, ReplicaId, RequestNumber, ViewNumber,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToReplica {
    // we skip sign/verify request messages, following convention
    // besides performance issue, it is also hard to register client's identity
    // at runtime
    Request(Request),
    RelayedRequest(Request), // remote address ignored

    PrePrepare(SignedMessage<PrePrepare>), // request batch piggybacked
    Prepare(SignedMessage<Prepare>),
    Commit(SignedMessage<Commit>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub op: Opaque,
    pub request_number: RequestNumber,
    pub client_id: ClientId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reply {
    pub view_number: ViewNumber,
    pub request_number: RequestNumber,
    pub client_id: ClientId, // following pbft paper, but why we need this?
    pub replica_id: ReplicaId,
    pub result: Opaque,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrePrepare {
    pub view_number: ViewNumber,
    pub op_number: OpNumber,
    pub digest: Digest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prepare {
    pub view_number: ViewNumber,
    pub op_number: OpNumber,
    pub digest: Digest,
    pub replica_id: ReplicaId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Commit {
    pub view_number: ViewNumber,
    pub op_number: OpNumber,
    pub digest: Digest,
    pub replica_id: ReplicaId,
}
