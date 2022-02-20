use crate::common::Opaque;

#[derive(Debug, Default)]
pub struct App {
    //
}

impl crate::App for App {
    fn execute(&mut self, op: Opaque) -> Opaque {
        let mut result = b"reply: ".to_vec();
        result.extend_from_slice(&op);
        result
    }
}
