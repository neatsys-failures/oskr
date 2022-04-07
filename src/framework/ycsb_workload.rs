use std::{
    collections::{HashMap, HashSet},
    iter::repeat_with,
};

use rand::{
    distributions::{uniform::SampleUniform, Uniform},
    thread_rng, Rng,
};

use crate::{
    app::ycsb::Op,
    common::{serialize, Opaque},
    facade::Invoke,
};

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
    pub min_scan_length: usize,
    pub max_scan_length: usize,
    pub scan_length_distribution: Distribution,
    pub insert_start: u64,
    pub insert_count: u64,
    pub zero_padding: usize,
    pub insert_order: Order,
    pub field_name_prefix: String,

    pub table: String,
    pub record_count: u64,
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
            record_count: 0, // expect user to override
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    key_chooser: Box<dyn FnMut() -> u64>,
    operation_chooser: Box<dyn FnMut() -> OpKind>,
    scan_length: Box<dyn FnMut() -> usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpKind {
    Read,
    Update,
    Insert,
    Scan,
    ReadModifyWrite,
}

impl Workload {
    fn uniform_generator<U: SampleUniform + Clone>(low: U, high: U) -> impl FnMut() -> U {
        let mut rng = thread_rng();
        move || rng.sample(Uniform::new(low.clone(), high.clone()))
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

    fn get_field_length_generator(property: &Property) -> impl FnMut() -> usize {
        let field_length = property.field_length;
        // TODO other distributions
        move || field_length
    }

    fn create_operation_generator(property: &Property) -> impl FnMut() -> OpKind {
        let mut low = 0.0;
        let mut predicate_list = Vec::new();
        let mut add_value = move |predicate_list: &mut Vec<_>, proportion, value: OpKind| {
            predicate_list.push(Box::new(move |p| {
                if (low..low + proportion).contains(&p) {
                    Some(value)
                } else {
                    None
                }
            }));
            low += proportion;
        };

        if property.read_proportion > 0.0 {
            add_value(&mut predicate_list, property.read_proportion, OpKind::Read);
        }
        if property.update_proportion > 0.0 {
            add_value(
                &mut predicate_list,
                property.update_proportion,
                OpKind::Update,
            );
        }
        if property.insert_proportion > 0.0 {
            add_value(
                &mut predicate_list,
                property.insert_proportion,
                OpKind::Insert,
            );
        }
        if property.scan_proportion > 0.0 {
            add_value(&mut predicate_list, property.scan_proportion, OpKind::Scan);
        }
        if property.read_modify_write_proportion > 0.0 {
            add_value(
                &mut predicate_list,
                property.read_modify_write_proportion,
                OpKind::ReadModifyWrite,
            );
        }

        let mut rng = thread_rng();
        move || {
            let p = rng.gen();
            for predicate in &predicate_list {
                if let Some(op_kind) = predicate(p) {
                    return op_kind;
                }
            }
            unreachable!()
        }
    }

    fn build_single_value(&mut self, key: &str) -> HashMap<String, Opaque> {
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

    fn build_value_table(&mut self, key: &str) -> HashMap<String, Opaque> {
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

    pub fn new(mut property: Property) -> Self {
        assert_ne!(property.record_count, 0);
        if property.insert_count == 0 {
            property.insert_count = property.record_count - property.insert_start;
        }

        let key_chooser = if property.request_distribution == Distribution::Uniform {
            Box::new(Self::uniform_generator(
                property.insert_start,
                property.insert_start + property.insert_count,
            ))
        } else {
            todo!()
        };
        let scan_length = match property.scan_length_distribution {
            Distribution::Uniform => Box::new(Self::uniform_generator(
                property.min_scan_length,
                property.max_scan_length,
            )),
            Distribution::Zipfian => todo!(),
            _ => unreachable!(),
        };
        Self {
            field_name_list: (0..property.field_count)
                .map(|i| format!("field{}", i))
                .collect(),
            field_chooser: Box::new(Self::uniform_generator(0, property.field_count)),
            field_length_generator: Box::new(Self::get_field_length_generator(&property)),
            key_sequence: Box::new(Self::counter_generator(property.insert_start)),
            key_chooser,
            operation_chooser: Box::new(Self::create_operation_generator(&property)),
            scan_length,
            property,
        }
    }

    fn next_key_number(&mut self) -> u64 {
        if self.property.request_distribution == Distribution::Exponential {
            todo!()
        } else if self.property.insert_proportion != 0.0 {
            todo!()
        } else {
            let mut key;
            while {
                key = (self.key_chooser)();
                key >= self.property.record_count
            } {}
            key
        }
    }

    pub async fn next(&mut self, client: &mut dyn Invoke, load: bool) -> OpKind {
        async fn invoke(client: &mut dyn Invoke, op: Op) {
            let mut buffer = Vec::new();
            serialize(op)(&mut buffer);
            client.invoke(buffer).await;
        }

        if load {
            let key_number = (self.key_sequence)();
            assert!(key_number < self.property.insert_start + self.property.insert_count);
            let key = Self::build_key_name(
                key_number,
                self.property.zero_padding,
                self.property.insert_order,
            );
            let value_table = self.build_value_table(&key);
            invoke(
                client,
                Op::Insert(self.property.table.clone(), key, value_table),
            )
            .await;
            return OpKind::Insert;
        }

        let op_kind = (self.operation_chooser)();
        let key = if op_kind != OpKind::Insert {
            let key_number = self.next_key_number();
            Self::build_key_name(
                key_number,
                self.property.zero_padding,
                self.property.insert_order,
            )
        } else {
            todo!()
        };
        let field_set = if !self.property.read_all_field {
            let field = (self.field_chooser)();
            [self.field_name_list[field].clone()].into_iter().collect()
        } else {
            HashSet::new()
        };
        let value_table = if self.property.write_all_field {
            self.build_value_table(&key)
        } else {
            self.build_single_value(&key)
        };
        let table = self.property.table.clone();

        match op_kind {
            OpKind::Read => {
                invoke(client, Op::Read(table, key, field_set)).await;
            }
            OpKind::ReadModifyWrite => {
                invoke(client, Op::Read(table.clone(), key.clone(), field_set)).await;
                invoke(client, Op::Update(table, key, value_table)).await;
            }
            OpKind::Scan => {
                let len = (self.scan_length)();
                invoke(client, Op::Scan(table, key, len, field_set)).await;
            }
            OpKind::Update => {
                invoke(client, Op::Update(table, key, value_table)).await;
            }
            OpKind::Insert => {
                todo!()
            }
        }
        op_kind
    }
}
