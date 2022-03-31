use std::time::Duration;

use bincode::Options;
use futures::future::join_all;
use tokio::{spawn, time::timeout};

use crate::{
    app::mock::App,
    common::{Opaque, SignedMessage, SigningKey},
    facade::{Invoke, Receiver},
    framework::tokio::AsyncEcosystem,
    protocol::pbft::message::{self, ToReplica},
    simulated::Transport,
    stage::Handle,
    tests::TRACING,
};

use super::{Client, Replica};

#[test]
fn send_pre_prepare_message() {
    *TRACING;
    let signing_key = SigningKey::from_bytes(&[1; 32]).unwrap();

    let pre_prepare = message::PrePrepare {
        view_number: 0,
        op_number: 1,
        digest: Default::default(),
    };
    let buffer = bincode::options()
        .serialize(&ToReplica::PrePrepare(SignedMessage::sign(
            pre_prepare,
            &signing_key,
        )))
        .unwrap();

    let message: ToReplica = bincode::options().deserialize_from(&buffer[..]).unwrap();
    if let ToReplica::PrePrepare(pre_prepare) = message {
        pre_prepare.verify(&signing_key.verifying_key()).unwrap();
    } else {
        panic!();
    }
}

fn generate_route(
    replica_list: &[Handle<Replica<Transport>>],
    client_list: &[Client<Transport, AsyncEcosystem>],
) {
    for replica in replica_list {
        replica.with_stateful(|replica| {
            replica.route_table.extend(
                client_list
                    .iter()
                    .map(|client| (client.id, client.get_address().clone())),
            );
        });
    }
}

#[tokio::test(start_paused = true)]
async fn one_request() {
    *TRACING;
    let config = Transport::config_builder(4, 1);
    let mut transport = Transport::new(config());
    let replica: Vec<_> = (0..4)
        .map(|i| Replica::register_new(config(), &mut transport, i, App::default(), 1, false))
        .collect();
    let client: Client<_, AsyncEcosystem> = Client::register_new(config(), &mut transport);
    let client = [client];
    generate_route(&replica, &client);
    let mut client = client.into_iter().next().unwrap();

    spawn(async move { transport.deliver(Duration::from_micros(1)).await });
    assert_eq!(
        timeout(Duration::from_micros(1), client.invoke(b"hello".to_vec()))
            .await
            .unwrap(),
        b"reply: hello".to_vec()
    );
}

#[tokio::test(start_paused = true)]
async fn multiple_client() {
    *TRACING;
    let config = Transport::config_builder(4, 1);
    let mut transport = Transport::new(config());
    let replica: Vec<_> = (0..4)
        .map(|i| Replica::register_new(config(), &mut transport, i, App::default(), 1, false))
        .collect();
    let client: Vec<Client<_, AsyncEcosystem>> = (0..3)
        .map(|_| Client::register_new(config(), &mut transport))
        .collect();
    generate_route(&replica, &client);
    let client: Vec<_> = client
        .into_iter()
        .enumerate()
        .map(|(i, mut client)| {
            spawn(async move {
                assert_eq!(
                    client.invoke(format!("client-{}", i).into()).await,
                    Opaque::from(format!("reply: client-{}", i))
                );
            })
        })
        .collect();

    spawn(async move { transport.deliver(Duration::from_micros(1)).await });
    timeout(Duration::from_micros(1), join_all(client))
        .await
        .unwrap();
}
