use serde_derive::{Deserialize, Serialize};

use crate::common::{
    ClientId, Digest, OpNumber, Opaque, ReplicaId, RequestNumber, SignedMessage, ViewNumber,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToReplica {
    Request(Request),
    OrderRequest(SignedMessage<OrderRequest>, Vec<Request>),
    Commit(Commit),
    ConfirmRequest(SignedMessage<ConfirmRequest>),
    Checkpoint(SignedMessage<Checkpoint>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToClient {
    SpeculativeResponse(
        SignedMessage<SpeculativeResponse>,
        ReplicaId,
        Opaque,
        SignedMessage<OrderRequest>,
    ),
    LocalCommit(SignedMessage<LocalCommit>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub op: Opaque,
    pub request_number: RequestNumber,
    pub client_id: ClientId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderRequest {
    pub view_number: ViewNumber,
    pub op_number: OpNumber,
    pub history_digest: Digest,
    pub digest: Digest,
    // non-deterministic field omitted
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeculativeResponse {
    pub view_number: ViewNumber,
    pub op_number: OpNumber,
    pub history_digest: Digest,
    pub digest: Digest,
    pub client_id: ClientId,
    pub request_number: RequestNumber,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Commit {
    pub client_id: ClientId,
    pub certification: Vec<(ReplicaId, SignedMessage<SpeculativeResponse>)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalCommit {
    pub view_number: ViewNumber,
    pub digest: Digest,
    pub history_digest: Digest,
    pub replica_id: ReplicaId,
    pub client_id: ClientId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfirmRequest {
    pub view_number: ViewNumber,
    pub request: Request,
    pub replica_id: ReplicaId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub op_number: OpNumber,
    pub digest: Digest,
    // omit application snapshot, we don't have application support this
}
