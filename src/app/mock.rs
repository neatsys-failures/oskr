use crate::{
    common::{OpNumber, Opaque},
    facade,
};

#[derive(Debug, Default)]
pub struct App {
    //
}

impl facade::App for App {
    fn execute(&mut self, _op_number: OpNumber, op: Opaque) -> Opaque {
        let mut result = b"reply: ".to_vec();
        result.extend_from_slice(&op);
        result
    }
}
