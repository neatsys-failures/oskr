use std::{
    collections::{BTreeMap, HashMap, HashSet},
    iter::repeat,
    path::Path,
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
    pub fn open(path: impl AsRef<Path>) -> Self {
        Self(Connection::open(path).unwrap())
    }

    pub fn create_table(&self, table: &str, field_set: &[&str]) {
        self.0
            .execute(&format!("DROP TABLE IF EXISTS {};", table), [])
            .unwrap(); // not very necessary here
        self.0
            .execute(
                &format!(
                    "CREATE TABLE {} (YCSB_KEY VARCHAR PRIMARY KEY, {});",
                    table,
                    field_set
                        .iter()
                        .map(|field| format!("{} TEXT", field))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                [],
            )
            .unwrap();
    }

    fn queried_field(field_set: HashSet<String>, column_list: Vec<&str>) -> Vec<String> {
        if !field_set.is_empty() {
            field_set.into_iter().collect()
        } else {
            column_list
                .into_iter()
                .filter(|field| *field != "YCSB_KEY")
                .map(|field| field.to_string())
                .collect()
        }
    }
    fn split_value_table(
        value_table: &HashMap<String, Opaque>,
        update: bool, // not a very good style
    ) -> (String, Vec<&dyn ToSql>) {
        let (stmt_list, param_list): (Vec<_>, Vec<_>) = value_table
            .iter()
            .map(|(field, value)| {
                (
                    if update {
                        format!("{}=?", field)
                    } else {
                        field.clone()
                    },
                    value as &dyn ToSql,
                )
            })
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
        result: &mut BTreeMap<String, Opaque>,
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
        result: &mut Vec<BTreeMap<String, Opaque>>,
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
            let mut value_table = BTreeMap::new();
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
        let (stmt_field, mut param_list) = Self::split_value_table(&value_table, true);
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
        let (stmt_field, mut param_list) = Self::split_value_table(&value_table, false);
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

#[cfg(test)]
mod tests {
    use crate::app::ycsb::Database as _;

    use super::*;

    #[test]
    fn create_table() {
        let db = Database::default();
        db.create_table("usertable", &["FIELD0", "FIELD1", "FIELD2"]);
    }

    #[test]
    fn insert_read() {
        let mut db = Database::default();
        db.create_table("usertable", &["A", "B", "C"]);
        db.insert(
            "usertable".to_string(),
            "id-0".to_string(),
            [
                ("A".to_string(), "a-0".as_bytes().to_vec()),
                ("B".to_string(), "b-0".as_bytes().to_vec()),
                ("C".to_string(), "c-0".as_bytes().to_vec()),
            ]
            .into_iter()
            .collect(),
        )
        .unwrap();
        let mut result = BTreeMap::new();
        db.read(
            "usertable".to_string(),
            "id-0".to_string(),
            ["A".to_string(), "B".to_string()].into_iter().collect(),
            &mut result,
        )
        .unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result["A"], "a-0".as_bytes());
        assert_eq!(result["B"], "b-0".as_bytes());
    }

    #[test]
    fn update() {
        let mut db = Database::default();
        db.create_table("usertable", &["A", "B", "C"]);
        db.insert(
            "usertable".to_string(),
            "id-0".to_string(),
            [
                ("A".to_string(), "a-0".as_bytes().to_vec()),
                ("B".to_string(), "b-0".as_bytes().to_vec()),
                ("C".to_string(), "c-0".as_bytes().to_vec()),
            ]
            .into_iter()
            .collect(),
        )
        .unwrap();
        db.update(
            "usertable".to_string(),
            "id-0".to_string(),
            [
                ("A".to_string(), "a-1".as_bytes().to_vec()),
                ("B".to_string(), "b-1".as_bytes().to_vec()),
                ("C".to_string(), "c-1".as_bytes().to_vec()),
            ]
            .into_iter()
            .collect(),
        )
        .unwrap();
        let mut result = BTreeMap::new();
        db.read(
            "usertable".to_string(),
            "id-0".to_string(),
            HashSet::new(),
            &mut result,
        )
        .unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result["A"], "a-1".as_bytes());
        assert_eq!(result["B"], "b-1".as_bytes());
        assert_eq!(result["C"], "c-1".as_bytes());
    }
}
