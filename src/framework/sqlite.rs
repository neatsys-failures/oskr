use std::{
    collections::{HashMap, HashSet},
    iter::repeat,
};

use rusqlite::{params, Connection, ToSql};

use crate::{
    app::ycsb::{self, DatabaseResult, Error},
    common::Opaque,
};

#[derive(Debug)]
pub struct Database(Connection);
impl Default for Database {
    fn default() -> Self {
        Self(Connection::open_in_memory().unwrap())
    }
}

impl Database {
    fn queried_field(field_set: HashSet<String>, column_list: Vec<&str>) -> Vec<String> {
        if !field_set.is_empty() {
            field_set.into_iter().collect()
        } else {
            column_list
                .into_iter()
                .map(|field| field.to_string())
                .collect()
        }
    }
    fn split_value_table(value_table: &HashMap<String, Opaque>) -> (String, Vec<&dyn ToSql>) {
        let (stmt_list, param_list): (Vec<_>, Vec<_>) = value_table
            .iter()
            .map(|(field, value)| (format!("{}=?", field), value as &dyn ToSql))
            .unzip();
        (stmt_list.join(", "), param_list)
    }
}

impl ycsb::Database for Database {
    fn read(
        &mut self,
        table: String,
        key: String,
        field_set: HashSet<String>,
        result: &mut HashMap<String, Opaque>,
    ) -> DatabaseResult {
        let mut stmt = self
            .0
            .prepare_cached(&format!("SELECT * FROM {} WHERE YCSB_KEY = ?", table))
            .unwrap();
        let field_list = Self::queried_field(field_set, stmt.column_names());
        let mut row_list = stmt.query([key]).unwrap();
        if let Some(row) = row_list.next().unwrap() {
            for field in field_list {
                result.insert(field.clone(), row.get_unwrap(&*field));
            }
            Ok(())
        } else {
            Err(Error::NotFound)
        }
    }
    fn scan(
        &mut self,
        table: String,
        start_key: String,
        record_count: usize,
        field_set: HashSet<String>,
        result: &mut Vec<HashMap<String, Opaque>>,
    ) -> DatabaseResult {
        let mut stmt = self
            .0
            .prepare_cached(&format!(
                "SELECT * FROM {} WHERE YCSB_KEY >= ? ORDER BY YCSB_KEY LIMIT ?",
                table
            ))
            .unwrap();
        let field_list = Self::queried_field(field_set, stmt.column_names());
        let mut row_list = stmt.query(params![start_key, record_count]).unwrap();
        while let Some(row) = row_list.next().unwrap() {
            if result.len() == record_count {
                break;
            }
            let mut value_table = HashMap::new();
            for field in &field_list {
                value_table.insert(field.clone(), row.get_unwrap(&**field));
            }
            result.push(value_table);
        }
        Ok(())
    }
    fn update(
        &mut self,
        table: String,
        key: String,
        value_table: HashMap<String, Opaque>,
    ) -> DatabaseResult {
        let (stmt_field, mut param_list) = Self::split_value_table(&value_table);
        param_list.push(&key as &dyn ToSql);
        let result = self
            .0
            .prepare_cached(&format!(
                "UPDATE {} SET {} WHERE YCSB_KEY = ?",
                table, stmt_field,
            ))
            .unwrap()
            .execute(&*param_list)
            .unwrap();
        if result == 1 {
            Ok(())
        } else {
            Err(Error::UnexpectedState)
        }
    }
    fn insert(
        &mut self,
        table: String,
        key: String,
        value_table: HashMap<String, Opaque>,
    ) -> DatabaseResult {
        // i did not see a real batch statement (i.e. merger) in rusqlite,
        // so just skip implementing batching
        let (stmt_field, mut param_list) = Self::split_value_table(&value_table);
        param_list.insert(0, &key as &dyn ToSql);
        let result = self
            .0
            .prepare_cached(&format!(
                "INSERT INTO {} (YCSB_KEY, {}) VALUES(?, {})",
                table,
                stmt_field,
                repeat("?")
                    .take(value_table.len())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
            .unwrap()
            .execute(&*param_list)
            .unwrap();
        if result == 1 {
            Ok(())
        } else {
            Err(Error::UnexpectedState)
        }
    }
    fn delete(&mut self, table: String, key: String) -> DatabaseResult {
        let result = self
            .0
            .prepare_cached(&format!("DELETE FROM {} WHERE YCSB_KEY = ?", table))
            .unwrap()
            .execute([key])
            .unwrap();
        if result == 1 {
            Ok(())
        } else {
            Err(Error::UnexpectedState)
        }
    }
}
