use std::{
    collections::{HashMap, HashSet},
    marker::PhantomData,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use futures::{
    channel::mpsc::{unbounded, UnboundedReceiver},
    select, FutureExt, StreamExt,
};
use tracing::{debug, warn};

use crate::{
    common::{
        deserialize, generate_id, serialize, ClientId, Config, Digest, OpNumber, Opaque, ReplicaId,
        RequestNumber, SignedMessage, ViewNumber,
    },
    facade::{AsyncEcosystem, Invoke, Receiver, Transport, TxAgent},
    protocol::zyzzyva::message::{self, ToReplica},
};

use super::message::ToClient;

pub struct Client<T: Transport, E> {
    address: T::Address,
    pub(super) id: ClientId,
    config: Config<T>,
    transport: T::TxAgent,
    rx: UnboundedReceiver<(T::Address, T::RxBuffer)>,
    _executor: PhantomData<E>,

    request_number: RequestNumber,
    view_number: ViewNumber,
    response_table: HashMap<ReplicaId, Response>,
    certification: Option<(
        Opaque,
        Digest,
        Digest,
        Vec<(ReplicaId, SignedMessage<message::SpeculativeResponse>)>,
    )>,
    commit_set: HashSet<ReplicaId>,
    wait_all: bool,
}

struct Response {
    signed: SignedMessage<message::SpeculativeResponse>,
    view_number: ViewNumber,
    op_number: OpNumber,
    history_digest: Digest,
    digest: Digest,
    // client id and request number is compared on the fly so omitted
    result: Opaque,
}

impl<T: Transport, E> Receiver<T> for Client<T, E> {
    fn get_address(&self) -> &T::Address {
        &self.address
    }
}

impl<T: Transport, E> Client<T, E> {
    pub fn register_new(config: Config<T>, transport: &mut T, wait_all: bool) -> Self {
        let (tx, rx) = unbounded();
        let client = Self {
            address: transport.ephemeral_address(),
            id: generate_id(),
            config,
            transport: transport.tx_agent(),
            rx,
            request_number: 0,
            view_number: 0,
            response_table: HashMap::new(),
            certification: None,
            commit_set: HashSet::new(),
            wait_all,
            _executor: PhantomData,
        };
        transport.register(&client, move |remote, buffer| {
            if tx.unbounded_send((remote, buffer)).is_err() {
                debug!("client channel broken");
            }
        });
        client
    }
}

#[async_trait]
impl<T: Transport, E: AsyncEcosystem<Opaque>> Invoke for Client<T, E>
where
    Self: Send + Sync,
    E: Send + Sync,
{
    async fn invoke(&mut self, op: Opaque) -> Opaque {
        self.request_number += 1;
        let request = message::Request {
            op,
            request_number: self.request_number,
            client_id: self.id,
        };
        let primary = self.config.view_primary(self.view_number);
        self.transport.send_message(
            self,
            self.config.replica(primary),
            serialize(ToReplica::Request(request.clone())),
        );

        // Zyzzyva paper is a little bit complicated on client side timers
        // there should be at least two ways to trigger broadcast resending,
        // i.e. step 4c. depends on how many spec response we got:
        // * with less than 2f + 1 responses, no commit sending, resend is
        //   triggered by the "second timer" of sending request
        //
        //   because client "resets its timers" after resending request, commit
        //   timer is refreshed as well to schedule another 2f + 1 check later
        // * with at least 2f + 1 responses, commit is sending instead of
        //   resending request, and client "starts a timer" (assuming that is
        //   commit resend timer) which will trigger request resending
        //
        //   Although paper not talks about, probably client should keep
        //   resending commit even start to resend request, so liveness is hold
        //   when both network is bad and someone is bad
        // Since the paper not specify interval of any timer, this
        // implementation takes the following approach so its behavior should
        // match above description on certain interval combination:
        // * resend timer triggers resending request and refresh commit timer
        //   every time
        // * commit timer don't refresh itself, and don't do anything if there
        //   is less than 2f + 1 replies
        // the timer detail may influence client strategy significantly which
        // results in major difference of overall system performance. hope
        // this do not hurt our reproducible :|
        let mut commit_timeout = Instant::now()
            + if !self.wait_all {
                Duration::from_millis(100)
            } else {
                Duration::from_secs(1000000)
            };
        let mut resend_timeout = Instant::now() + Duration::from_millis(1000);
        loop {
            select! {
                recv = self.rx.next() => {
                    let (remote, buffer) = recv.unwrap();
                    if let Some(result) = self.receive_buffer(remote, buffer) {
                        // clean up
                        self.response_table.clear();
                        self.certification = None;
                        self.commit_set.clear();
                        return result;
                    }
                    if !self.wait_all {
                        self.send_commit();
                    }
                }
                _ = E::sleep_until(resend_timeout).fuse() => {
                    if self.request_number > 1 {
                        warn!("resend for request number {}, commit: {}", self.request_number, self.certification.is_some());
                    } else {
                        debug!("resend for request number {}", self.request_number);
                    }
                    self.transport
                        .send_message_to_all(self, self.config.replica(..), serialize(ToReplica::Request(request.clone())));
                    commit_timeout = Instant::now() + Duration::from_millis(100);
                    resend_timeout = Instant::now() + Duration::from_millis(1000);
                }
                _ = E::sleep_until(commit_timeout).fuse() => {
                    if self.request_number > 1 {
                        warn!("commit timeout for request {}", self.request_number);
                    } else {
                        debug!("commit timeout for request {}", self.request_number);
                    }
                    // set next commit to "forever" (will be set back in resend)
                    self.send_commit();
                    commit_timeout = Instant::now() + Duration::from_secs(1000000); // TODO
                }
            }
        }
    }
}

impl<T: Transport, A> Client<T, A> {
    fn receive_buffer(&mut self, _remote: T::Address, buffer: T::RxBuffer) -> Option<Opaque> {
        match deserialize(buffer.as_ref()).unwrap() {
            // TODO proof of misbehavior (not really planned actually)
            ToClient::SpeculativeResponse(response, replica_id, result, _order_request) => {
                let (response, signed) = (response.assume_verified(), response);
                debug!(
                    "[{:?}] spec request number {} replica id {}",
                    self.id, response.request_number, replica_id
                );
                if (response.client_id, response.request_number) != (self.id, self.request_number) {
                    return None;
                }
                self.response_table.insert(
                    replica_id,
                    Response {
                        signed,
                        view_number: response.view_number,
                        op_number: response.op_number,
                        history_digest: response.history_digest,
                        digest: response.digest,
                        result: result.clone(),
                    },
                );
                // TODO save order request message
                if response.view_number > self.view_number {
                    self.view_number = response.view_number;
                }
                let response0 = response;
                let certification: Vec<_> = self
                    .response_table
                    .iter()
                    .filter(|(_, response)| {
                        response.view_number == response0.view_number
                            && response.op_number == response0.op_number
                            && response.history_digest == response0.history_digest
                            && response.digest == response0.digest
                            && response.result == result
                    })
                    .collect();
                // debug!("cert size {}", certification.len());
                if certification.len() == 3 * self.config.f + 1 {
                    self.view_number = response0.view_number;
                    return Some(result);
                } else if certification.len() >= 2 * self.config.f + 1
                    && self.certification.is_none()
                {
                    self.certification = Some((
                        result,
                        response0.digest,
                        response0.history_digest,
                        certification
                            .into_iter()
                            .take(2 * self.config.f + 1)
                            .map(|(replica_id, response)| (*replica_id, response.signed.clone()))
                            .collect(),
                    ));
                    self.view_number = response0.view_number;
                }
                None
            }
            ToClient::LocalCommit(commit) => {
                if let Some((result, digest, history_digest, _)) = &self.certification {
                    let commit = commit.assume_verified();
                    if (
                        commit.client_id,
                        commit.view_number,
                        commit.digest,
                        commit.history_digest,
                    ) != (self.id, self.view_number, *digest, *history_digest)
                    {
                        return None;
                    }
                    self.commit_set.insert(commit.replica_id);
                    if self.commit_set.len() == 2 * self.config.f + 1 {
                        return Some(result.clone());
                    }
                }
                None
            }
        }
    }

    fn send_commit(&self) {
        if let Some((_, _, _, certification)) = &self.certification {
            let commit = message::Commit {
                client_id: self.id,
                certification: certification.clone(),
            };
            self.transport.send_message_to_all(
                self,
                self.config.replica(..),
                serialize(ToReplica::Commit(commit)),
            );
        }
    }
}
