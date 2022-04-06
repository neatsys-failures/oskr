use std::collections::{HashMap, HashSet};

use serde_derive::{Deserialize, Serialize};

use crate::{
    common::{deserialize, serialize, OpNumber, Opaque},
    facade::App,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Op {
    Read(String, String, HashSet<String>),
    Scan(String, String, usize, HashSet<String>),
    Update(String, String, HashMap<String, Opaque>),
    Insert(String, String, HashMap<String, Opaque>),
    Delete(String, String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Result {
    ReadOk(HashMap<String, Opaque>),
    ScanOk(Vec<HashMap<String, Opaque>>),
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
        result: &mut HashMap<String, Opaque>,
    ) -> DatabaseResult;
    fn scan(
        &mut self,
        table: String,
        start_key: String,
        record_count: usize,
        field_set: HashSet<String>,
        result: &mut Vec<HashMap<String, Opaque>>,
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
                let mut result = HashMap::new();
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
