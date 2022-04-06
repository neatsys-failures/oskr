use std::collections::{BTreeMap, HashMap, HashSet};

use serde_derive::{Deserialize, Serialize};
use tracing::warn;

use crate::common::{deserialize, serialize, OpNumber, Opaque};

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
}

pub struct App {
    storage: BTreeMap<String, HashMap<String, Opaque>>,
    // speculative support?
}

impl crate::facade::App for App {
    fn execute(&mut self, _op_number: OpNumber, op: Opaque) -> Opaque {
        let result = match deserialize(&*op).unwrap() {
            Op::Read(_table, key, field_set) => {
                let mut result = if let Some(record) = self.storage.get(&key) {
                    record.clone()
                } else {
                    warn!("key not found: {}", key);
                    HashMap::new()
                };
                result.retain(|field, _| field_set.is_empty() || field_set.contains(field));
                Result::ReadOk(result)
            }
            Op::Scan(_table, start_key, record_count, field_set) => {
                let result = self
                    .storage
                    .range(start_key..)
                    .take(record_count)
                    .map(|(_, record)| {
                        let mut record = record.clone();
                        record.retain(|field, _| field_set.is_empty() || field_set.contains(field));
                        record
                    })
                    .collect();
                Result::ScanOk(result)
            }
            Op::Update(_table, key, value_table) => {
                self.storage.entry(key).or_default().extend(value_table);
                Result::UpdateOk
            }
            Op::Insert(_table, key, value_table) => {
                self.storage.insert(key, value_table);
                Result::InsertOk
            }
            Op::Delete(_table, key) => {
                self.storage.remove(&key);
                Result::DeleteOk
            }
        };
        let mut buffer = Opaque::new();
        serialize(result)(&mut buffer);
        buffer
    }
}
