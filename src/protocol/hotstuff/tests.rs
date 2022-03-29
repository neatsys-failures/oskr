use std::time::Duration;

use tokio::{
    spawn,
    sync::oneshot,
    time::{sleep, timeout},
};

use crate::{
    app::mock::App, facade::Invoke, runtime::tokio::AsyncEcosystem, simulated::Transport,
    tests::TRACING,
};

use super::{Client, Replica};

#[tokio::test(start_paused = true)]
async fn single_request() {
    *TRACING;
    let mut transport = Transport::new(4, 1);
    let replica: Vec<_> = (0..4)
        .map(|i| Replica::register_new(&mut transport, i, App::default(), 1))
        .collect();
    let mut client: Client<_, AsyncEcosystem> = Client::register_new(&mut transport);

    let (stop_tx, stop) = oneshot::channel();
    spawn(async move { transport.deliver_until(stop).await });
    let request = spawn(async move { client.invoke(b"hello".to_vec()).await });

    for _ in 0..3 {
        sleep(Duration::from_millis(1)).await;
        replica[0].with_stateful(|replica| {
            replica.on_beat(Vec::new());
        });
    }
    assert_eq!(
        timeout(Duration::from_millis(1), request)
            .await
            .unwrap()
            .unwrap(),
        b"reply: hello".to_vec()
    );
    stop_tx.send(()).unwrap();
}
