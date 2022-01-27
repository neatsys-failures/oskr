use crate::model::{App as AppTrait, *};

pub struct App {
    //
}

impl AppTrait for App {
    fn execute(&mut self, op: &[u8]) -> Data {
        let mut result = b"reply: ".to_vec();
        result.extend_from_slice(op);
        result
    }
}
