use std::collections::{BTreeMap, HashMap, HashSet};

use crate::{
    app::ycsb::{self, DatabaseResult, Error},
    common::Opaque,
};

pub struct Database {
    // outer level B-Tree for supporting scan operation
    // inner level using B-Tree instead of hashed, because the database
    // interface is using B-Tree as well
    storage: BTreeMap<String, BTreeMap<String, Opaque>>,
}

impl ycsb::Database for Database {
    fn read(
        &mut self,
        _table: String,
        key: String,
        field_set: HashSet<String>,
        result: &mut BTreeMap<String, Opaque>,
    ) -> DatabaseResult {
        if let Some(row) = self.storage.get(&key) {
            *result = if field_set.is_empty() {
                row.clone()
            } else {
                row.iter()
                    .filter(|(column, _)| field_set.contains(*column))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            };
            Ok(())
        } else {
            Err(Error::NotFound)
        }
    }
    fn scan(
        &mut self,
        _table: String,
        start_key: String,
        record_count: usize,
        field_set: HashSet<String>,
        result: &mut Vec<BTreeMap<String, Opaque>>,
    ) -> DatabaseResult {
        *result = self
            .storage
            .range(start_key..)
            .take(record_count)
            .map(|(_, row)| {
                if field_set.is_empty() {
                    row.clone()
                } else {
                    row.iter()
                        .filter(|(column, _)| field_set.contains(*column))
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect()
                }
            })
            .collect();
        Ok(())
    }
    fn update(
        &mut self,
        _table: String,
        key: String,
        value_table: HashMap<String, Opaque>,
    ) -> DatabaseResult {
        if let Some(row) = self.storage.get_mut(&key) {
            row.extend(value_table);
            Ok(())
        } else {
            Err(Error::NotFound)
        }
    }
    fn insert(
        &mut self,
        _table: String,
        key: String,
        value_table: HashMap<String, Opaque>,
    ) -> DatabaseResult {
        self.storage.insert(key, value_table.into_iter().collect());
        Ok(())
    }
    fn delete(&mut self, _table: String, key: String) -> DatabaseResult {
        self.storage.remove(&key);
        Ok(())
    }
}
