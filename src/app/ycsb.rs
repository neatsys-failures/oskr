use std::collections::{BTreeMap, HashMap, HashSet};

use async_trait::async_trait;
use serde_derive::{Deserialize, Serialize};
use tracing::info;

use crate::{
    common::{deserialize, serialize, OpNumber, Opaque},
    facade::{App, Invoke, Receiver, Transport},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Op {
    Read(String, String, HashSet<String>),
    Scan(String, String, usize, HashSet<String>),
    Update(String, String, HashMap<String, Opaque>),
    Insert(String, String, HashMap<String, Opaque>),
    Delete(String, String),
}

// we are using BTreeMap instead of HashMap to maintain a deterministic order
// during serialization, so the serialized messages can match each other which
// is required by BFT protocols.
// we could also use vector of pair instead of map, but that is not what
// expected by database backends
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Result {
    ReadOk(BTreeMap<String, Opaque>),
    ScanOk(Vec<BTreeMap<String, Opaque>>),
    UpdateOk,
    InsertOk,
    DeleteOk,
    Error(Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Error {
    Failed,
    NotFound,
    NotImplemented,
    UnexpectedState,
    BadRequest,
    Forbidden,
    ServiceUnavailable,
    BatchedOk, // ...?
}

pub type DatabaseResult = std::result::Result<(), Error>;
pub trait Database {
    fn read(
        &mut self,
        table: String,
        key: String,
        field_set: HashSet<String>,
        result: &mut BTreeMap<String, Opaque>,
    ) -> DatabaseResult;
    fn scan(
        &mut self,
        table: String,
        start_key: String,
        record_count: usize,
        field_set: HashSet<String>,
        result: &mut Vec<BTreeMap<String, Opaque>>,
    ) -> DatabaseResult;
    fn update(
        &mut self,
        table: String,
        key: String,
        value_table: HashMap<String, Opaque>,
    ) -> DatabaseResult;
    fn insert(
        &mut self,
        table: String,
        key: String,
        value_table: HashMap<String, Opaque>,
    ) -> DatabaseResult;
    fn delete(&mut self, table: String, key: String) -> DatabaseResult;
}

// should we allow app do customized rollback?
impl<A: Database> App for A {
    fn execute(&mut self, _op_number: OpNumber, op: Opaque) -> Opaque {
        let result = match deserialize(&*op).unwrap() {
            Op::Read(table, key, field_set) => {
                let mut result = BTreeMap::new();
                if let Err(error) = self.read(table, key, field_set, &mut result) {
                    Result::Error(error)
                } else {
                    Result::ReadOk(result)
                }
            }
            Op::Scan(table, start_key, record_count, field_set) => {
                let mut result = Vec::new();
                if let Err(error) =
                    self.scan(table, start_key, record_count, field_set, &mut result)
                {
                    Result::Error(error)
                } else {
                    Result::ScanOk(result)
                }
            }
            Op::Update(table, key, value_table) => {
                if let Err(error) = self.update(table, key, value_table) {
                    Result::Error(error)
                } else {
                    Result::UpdateOk
                }
            }
            Op::Insert(table, key, value_table) => {
                if let Err(error) = self.insert(table, key, value_table) {
                    Result::Error(error)
                } else {
                    Result::InsertOk
                }
            }
            Op::Delete(table, key) => {
                if let Err(error) = self.delete(table, key) {
                    Result::Error(error)
                } else {
                    Result::DeleteOk
                }
            }
        };
        let mut buffer = Opaque::new();
        serialize(result)(&mut buffer);
        buffer
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TraceClient<T: Transport>(T::Address);
impl<T: Transport> Default for TraceClient<T> {
    fn default() -> Self {
        Self(T::null_address())
    }
}

#[async_trait]
impl<T: Transport> Invoke for TraceClient<T> {
    async fn invoke(&mut self, op: Opaque) -> Opaque {
        let op: Op = deserialize(&*op).unwrap();
        info!("{:?}", op);
        Opaque::default()
    }
}

impl<T: Transport> Receiver<T> for TraceClient<T> {
    fn get_address(&self) -> &T::Address {
        &self.0
    }
}
