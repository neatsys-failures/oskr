use std::{collections::HashMap, iter::repeat_with};

use rand::{distributions::Uniform, thread_rng, Rng};

use crate::{app::ycsb::Op, common::Opaque};

pub struct Property {
    pub field_count: usize,
    pub field_length: usize,
    pub min_field_length: u32,
    pub read_all_field: bool,
    pub write_all_field: bool,
    pub read_proportion: f32,
    pub update_proportion: f32,
    pub insert_proportion: f32,
    pub scan_proportion: f32,
    pub read_modify_write_proportion: f32,
    pub request_distribution: Distribution,
    pub min_scan_length: u32,
    pub max_scan_length: u32,
    pub scan_length_distribution: Distribution,
    pub insert_start: u64,
    pub insert_count: u64,
    pub zero_padding: usize,
    pub insert_order: Order,
    pub field_name_prefix: String,

    pub table: String,
}

impl Default for Property {
    fn default() -> Self {
        Self {
            field_count: 10,
            field_length: 100,
            min_field_length: 1,
            read_all_field: true,
            write_all_field: false,
            read_proportion: 0.95,
            update_proportion: 0.05,
            insert_proportion: 0.0,
            scan_proportion: 0.0,
            read_modify_write_proportion: 0.0,
            request_distribution: Distribution::Uniform,
            min_scan_length: 1,
            max_scan_length: 1000,
            scan_length_distribution: Distribution::Uniform,
            insert_start: 0,
            insert_count: 0,
            zero_padding: 1,
            insert_order: Order::Hashed,
            field_name_prefix: "field".to_string(),

            table: "usertable".to_string(),
        }
    }
}

pub enum Distribution {
    Uniform,
    Zipfian,
    Hotspot,
    Sequential,
    Exponential,
    Latest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    Ordered,
    Hashed,
}

pub struct Workload {
    property: Property,
    field_name_list: Vec<String>,
    field_chooser: Box<dyn FnMut() -> usize>,
    field_length_generator: Box<dyn FnMut() -> usize>,
    key_sequence: Box<dyn FnMut() -> u64>,
}

impl Workload {
    fn uniform_usize_generator(low: usize, high: usize) -> impl FnMut() -> usize {
        let mut rng = thread_rng();
        move || rng.sample(Uniform::new(low, high))
    }

    fn counter_generator(mut counter: u64) -> impl FnMut() -> u64 {
        move || {
            let n = counter;
            counter += 1;
            n
        }
    }

    fn build_key_name(mut key_number: u64, zero_padding: usize, order: Order) -> String {
        if order == Order::Hashed {
            key_number = Self::fnvhash64(key_number);
        }
        format!("user{key_number:0zero_padding$}")
    }

    fn fnvhash64(mut val: u64) -> u64 {
        const OFFSET_BASIS: u64 = 0xCBF29CE484222325;
        const PRIME: u64 = 1099511628211;

        let mut hashval = OFFSET_BASIS;
        for _ in 0..8 {
            let octet = val & 0x00ff;
            val >>= 8;
            hashval ^= octet;
            hashval = hashval.overflowing_mul(PRIME).0; // hopefully this equals to Java's signed multiply + abs
        }
        hashval
    }

    fn field_length_generator(property: &Property) -> impl FnMut() -> usize {
        let field_length = property.field_length;
        // TODO other distributions
        move || field_length
    }

    fn build_single_value(&mut self) -> HashMap<String, Opaque> {
        let mut value_table = HashMap::new();
        let field = (self.field_chooser)();
        let field_length = (self.field_length_generator)();
        let field = self.field_name_list[field].clone();
        let mut rng = thread_rng();
        value_table.insert(
            field,
            repeat_with(|| rng.gen()).take(field_length).collect(),
        );
        value_table
    }

    fn build_value_table(&mut self) -> HashMap<String, Opaque> {
        let mut value_table = HashMap::new();
        let mut rng = thread_rng();
        for field in self.field_name_list.clone() {
            let field_length = (self.field_length_generator)();
            value_table.insert(
                field,
                repeat_with(|| rng.gen()).take(field_length).collect(),
            );
        }
        value_table
    }

    pub fn new(property: Property) -> Self {
        Self {
            field_name_list: (0..property.field_count)
                .map(|i| format!("field{}", i))
                .collect(),
            field_chooser: Box::new(Self::uniform_usize_generator(0, property.field_count)),
            field_length_generator: Box::new(Self::field_length_generator(&property)),
            key_sequence: Box::new(Self::counter_generator(property.insert_start)),
            property,
        }
    }

    pub fn insert(mut self) -> impl Iterator<Item = Op> {
        (self.property.insert_start..self.property.insert_start + self.property.insert_count).map(
            move |_| {
                let key_number = (self.key_sequence)();
                let key = Self::build_key_name(
                    key_number,
                    self.property.zero_padding,
                    self.property.insert_order,
                );
                let value_table = self.build_value_table();
                Op::Insert(self.property.table.clone(), key, value_table)
            },
        )
    }
}
