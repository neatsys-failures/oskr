use crate::common::{
    ClientId, Digest, OpNumber, Opaque, ReplicaId, RequestNumber, SignedMessage, ViewNumber,
};

pub enum ToReplica {
    Request(Request),
    OrderRequest(SignedMessage<OrderRequest>), // + request message
    // is this really good design
    Commit(Commit),
    ConfirmRequest(SignedMessage<ConfirmRequest>),
    Checkpoint(SignedMessage<Checkpoint>),
}

pub enum ToClient {
    SpeculativeResponse(
        SignedMessage<SpeculativeResponse>,
        ReplicaId,
        Opaque,
        SignedMessage<OrderRequest>,
    ),
    LocalCommit(SignedMessage<LocalCommit>),
}

pub struct Request {
    pub op: Opaque,
    pub request_number: RequestNumber,
    pub client_id: ClientId,
}

pub struct OrderRequest {
    pub view_number: ViewNumber,
    pub op_number: OpNumber,
    pub history: Digest,
    pub digest: Digest,
    // non-deterministic field omitted
}

pub struct SpeculativeResponse {
    pub view_number: ViewNumber,
    pub op_number: OpNumber,
    pub history: Digest,
    pub digest: Digest,
    pub client_id: ClientId,
    pub request_number: RequestNumber,
}

pub struct Commit {
    pub client_id: ClientId,
    pub certification: Vec<(ReplicaId, SignedMessage<SpeculativeResponse>)>,
}

pub struct LocalCommit {
    pub view_number: ViewNumber,
    pub digest: Digest,
    pub history: Digest,
    pub replica_id: ReplicaId,
    pub client_id: ClientId,
}

pub struct ConfirmRequest {
    pub view_number: ViewNumber,
    pub request: Request,
    pub replica_id: ReplicaId,
}

pub struct Checkpoint {
    pub op_number: OpNumber,
    pub digest: Digest,
    // omit application snapshot, we don't have application support this
}
