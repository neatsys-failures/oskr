use std::time::Duration;

use bincode::Options;
use tokio::{spawn, time::timeout};

use crate::{
    app::mock::App,
    common::{SignedMessage, SigningKey},
    replication::pbft::message::{self, ToReplica},
    simulated::{AsyncExecutor, Transport},
    tests::TRACING,
    Invoke,
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

#[tokio::test(start_paused = true)]
async fn one_request() {
    *TRACING;
    let mut transport = Transport::new(4, 1);
    for i in 0..4 {
        Replica::register_new(&mut transport, i, App::default(), 1);
    }
    let mut client: Client<_, AsyncExecutor> = Client::register_new(&mut transport);

    // allow one resend for caching remote address

    spawn(async move { transport.deliver(Duration::from_millis(1001)).await });
    assert_eq!(
        timeout(
            Duration::from_millis(1001),
            client.invoke(b"hello".to_vec())
        )
        .await
        .unwrap(),
        b"reply: hello".to_vec()
    );
}
