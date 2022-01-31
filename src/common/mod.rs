use crate::model::*;
use bincode::serialize_into;
use rand::distributions::Alphanumeric;
use rand::*;
use serde::Serialize;
use std::io::Cursor;
use std::sync::Arc;

pub(crate) struct NullTransport;
impl Transport for NullTransport {
    fn deref_rx_buffer(&self, _: RxBufferData) -> &[u8] {
        unimplemented!()
    }
    fn drop_rx_buffer(&self, _: RxBufferData) {
        unimplemented!()
    }

    fn send_message(
        self: Arc<Self>,
        _: &dyn TransportReceiver,
        _: &TransportAddress,
        _: &mut dyn FnMut(&mut [u8]) -> u16,
    ) {
        unimplemented!()
    }
    fn send_message_to_replica(
        self: Arc<Self>,
        _: &dyn TransportReceiver,
        _: ReplicaId,
        _: &mut dyn FnMut(&mut [u8]) -> u16,
    ) {
        unimplemented!()
    }
    fn send_message_to_all(
        self: Arc<Self>,
        _: &dyn TransportReceiver,
        _: &mut dyn FnMut(&mut [u8]) -> u16,
    ) {
        unimplemented!()
    }
}

pub fn generate_id() -> ClientId {
    let mut rng = thread_rng();
    [
        rng.sample(Alphanumeric),
        rng.sample(Alphanumeric),
        rng.sample(Alphanumeric),
        rng.sample(Alphanumeric),
    ]
}

pub(crate) fn bincode<M: Serialize>(message: M) -> impl Fn(&mut [u8]) -> u16 {
    move |buffer| {
        let mut cursor = Cursor::new(buffer);
        serialize_into(&mut cursor, &message).unwrap();
        cursor.position() as _
    }
}
