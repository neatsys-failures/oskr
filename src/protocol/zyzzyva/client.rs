use std::{
    collections::HashMap,
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
}

impl<T: Transport, E> Receiver<T> for Client<T, E> {
    fn get_address(&self) -> &T::Address {
        &self.address
    }
}

impl<T: Transport, E> Client<T, E> {
    pub fn register_new(config: Config<T>, transport: &mut T) -> Self {
        let (tx, rx) = unbounded();
        let client = Self {
            address: transport.ephemeral_address(),
            id: generate_id(),
            config,
            transport: transport.tx_agent(),
            rx,
            request_number: 0,
            view_number: 0,
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

        struct Response {
            signed: SignedMessage<message::SpeculativeResponse>,
            view_number: ViewNumber,
            op_number: OpNumber,
            history_digest: Digest,
            digest: Digest,
            // client id and request number is compared on the fly so omitted
            result: Opaque,
        }
        let mut response_table = HashMap::new();
        enum Status {
            Committed(Opaque),
            Certified(Vec<(ReplicaId, SignedMessage<message::SpeculativeResponse>)>), // TODO
            Other,
        }
        let mut receive_buffer =
            move |client: &mut Self, _remote: T::Address, buffer: T::RxBuffer| {
                match deserialize(buffer.as_ref()).unwrap() {
                    // TODO proof of misbehavior (not really planned actually)
                    ToClient::SpeculativeResponse(response, replica_id, result, _order_request) => {
                        let (response, signed) = (response.assume_verified(), response);
                        if (response.client_id, response.request_number)
                            != (client.id, client.request_number)
                        {
                            return Status::Other;
                        }
                        response_table.insert(
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
                        if response.view_number > client.view_number {
                            client.view_number = response.view_number;
                        }
                        let response0 = response;
                        let certification: Vec<_> = response_table
                            .iter()
                            .filter(|(_, response)| {
                                response.view_number == response0.view_number
                                    && response.op_number == response0.op_number
                                    && response.history_digest == response0.history_digest
                                    && response.digest == response0.digest
                                    && response.result == result
                            })
                            .collect();
                        if certification.len() == 3 * client.config.f + 1 {
                            Status::Committed(result)
                        } else if certification.len() >= 2 * client.config.f + 1 {
                            Status::Certified(
                                certification
                                    .into_iter()
                                    .map(|(replica_id, response)| {
                                        (*replica_id, response.signed.clone())
                                    })
                                    .collect(),
                            )
                        } else {
                            Status::Other
                        }
                    }
                    ToClient::LocalCommit(commit) => todo!(),
                }
            };

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
        let mut commit_timeout = Instant::now() + Duration::from_millis(100);
        let mut resend_timeout = Instant::now() + Duration::from_millis(1000);
        let mut certification = None;
        loop {
            select! {
                recv = self.rx.next() => {
                    let (remote, buffer) = recv.unwrap();
                    match (receive_buffer(self, remote, buffer), &mut certification) {
                        (Status::Committed(result), _) => return result,
                        (Status::Certified(cert), None) => certification = Some(cert),
                        _ => {}
                    }
                }
                _ = E::sleep_until(resend_timeout).fuse() => {
                    warn!("resend for request number {}", self.request_number);
                    self.transport
                        .send_message_to_all(self, self.config.replica(..), serialize(ToReplica::Request(request.clone())));
                    resend_timeout = Instant::now() + Duration::from_millis(1000);
                    commit_timeout = Instant::now() + Duration::from_millis(100);
                }
                _ = E::sleep_until(commit_timeout).fuse() => {
                    warn!("commit timeout for request {}", self.request_number);
                    if let Some(certification) = &certification {
                        todo!()
                    }
                    commit_timeout = Instant::now() + Duration::from_secs(100); // TODO
                }
            }
        }
    }
}
