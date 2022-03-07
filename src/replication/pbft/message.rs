use serde_derive::{Deserialize, Serialize};

use crate::common::{
    signed::SignedMessage, ClientId, OpNumber, Opaque, ReplicaId, RequestNumber, ViewNumber,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReplicaMessage {
    // we skip sign/verify request and reply messages, following convention
    // besides performance issue, it is also hard to register client's identity
    // at runtime
    Request(Request),
    PrePrepare(SignedMessage<PrePrepare>, Vec<Request>),
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
    pub client_id: ClientId,
    pub replica_id: ReplicaId,
    pub result: Opaque,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrePrepare {
    pub view_number: ViewNumber,
    pub op_number: OpNumber,
    pub digest: [u8; 32],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prepare {
    pub view_number: ViewNumber,
    pub op_number: OpNumber,
    pub digest: [u8; 32],
    pub replica_id: ReplicaId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Commit {
    pub view_number: ViewNumber,
    pub op_number: OpNumber,
    pub digest: [u8; 32],
    pub replica_id: ReplicaId,
}
