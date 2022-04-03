use std::sync::Arc;

use crate::{
    common::{OpNumber, Opaque},
    facade,
};

pub enum Upcall {
    Execute(OpNumber, Opaque),
    Rollback(OpNumber, OpNumber, Vec<(OpNumber, Opaque)>),
    Commit(OpNumber),
}

pub struct App {
    execute_stub: Arc<dyn Fn(&mut App, OpNumber, Opaque) -> Opaque + Send + Sync>,
    pub upcall_log: Vec<Upcall>,
}

impl App {
    pub fn new(
        execute_stub: impl Fn(&mut App, OpNumber, Opaque) -> Opaque + Send + Sync + 'static,
    ) -> Self {
        Self {
            execute_stub: Arc::new(execute_stub),
            upcall_log: Vec::new(),
        }
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new(|_, _, op| {
            let mut result = b"reply: ".to_vec();
            result.extend_from_slice(&op);
            result
        })
    }
}

impl facade::App for App {
    fn execute(&mut self, op_number: OpNumber, op: Opaque) -> Opaque {
        self.upcall_log.push(Upcall::Execute(op_number, op.clone()));
        self.execute_stub.clone()(self, op_number, op)
    }

    fn rollback(
        &mut self,
        current: OpNumber,
        to: OpNumber,
        op_list: &mut dyn Iterator<Item = (OpNumber, Opaque)>,
    ) {
        self.upcall_log
            .push(Upcall::Rollback(current, to, op_list.collect()));
    }

    fn commit(&mut self, op_number: OpNumber) {
        self.upcall_log.push(Upcall::Commit(op_number));
    }
}
