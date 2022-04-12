use std::{
    collections::{hash_map::DefaultHasher, HashMap, HashSet},
    convert::Infallible,
    hash::{Hash, Hasher},
    iter::repeat_with,
    str::FromStr,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};

use rand::{
    distributions::{uniform::SampleUniform, Uniform},
    random, thread_rng, Rng,
};
use tracing::debug;

use crate::{
    app::ycsb::Op,
    common::{serialize, Opaque},
};

#[derive(Debug, Clone)]
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
    pub data_integrity: bool,
    pub do_transaction: bool,
    pub operation_count: usize,
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
            data_integrity: false,
            do_transaction: true,
            operation_count: 0,
        }
    }
}

impl Property {
    pub fn rewrite_load(&mut self) {
        self.do_transaction = false;
    }

    pub fn rewrite(&mut self, key: &str, value: &str) {
        match key {
            "workload" => assert_eq!(value, "site.ycsb.workloads.CoreWorkload"),
            "fieldcount" => self.field_count = value.parse().unwrap(),
            "fieldlength" => self.field_length = value.parse().unwrap(),
            "minfieldlength" => self.min_field_length = value.parse().unwrap(),
            "readallfields" => self.read_all_field = value.parse().unwrap(),
            "writeallfields" => self.write_all_field = value.parse().unwrap(),
            "readproportion" => self.read_proportion = value.parse().unwrap(),
            "updateproportion" => self.update_proportion = value.parse().unwrap(),
            "insertproportion" => self.insert_proportion = value.parse().unwrap(),
            "scanproportion" => self.scan_proportion = value.parse().unwrap(),
            "readmodifywriteproportion" => {
                self.read_modify_write_proportion = value.parse().unwrap()
            }
            "requestdistribution" => self.request_distribution = value.parse().unwrap(),
            "minscanlength" => self.min_scan_length = value.parse().unwrap(),
            "maxscanlength" => self.max_scan_length = value.parse().unwrap(),
            "scanlengthdistribution" => self.scan_length_distribution = value.parse().unwrap(),
            "insertstart" => self.insert_start = value.parse().unwrap(),
            "insertcount" => self.insert_count = value.parse().unwrap(),
            "zeropadding" => self.zero_padding = value.parse().unwrap(),
            "insertorder" => self.insert_order = value.parse().unwrap(),
            "fieldnameprefix" => self.field_name_prefix = value.parse().unwrap(),
            "table" => self.table = value.parse().unwrap(),
            "recordcount" => self.record_count = value.parse().unwrap(),
            "dataintegrity" => self.data_integrity = value.parse().unwrap(),
            "operationcount" => self.operation_count = value.parse().unwrap(),
            _ => unreachable!(),
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
impl FromStr for Distribution {
    type Err = Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "uniform" => Self::Uniform,
            "zipfian" => Self::Zipfian,
            "hotspot" => Self::Hotspot,
            "sequential" => Self::Sequential,
            "exponential" => Self::Exponential,
            "latest" => Self::Latest,
            _ => unreachable!(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    Ordered,
    Hashed,
}
impl FromStr for Order {
    type Err = Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "ordered" => Self::Ordered,
            "hashed" => Self::Hashed,
            _ => unreachable!(),
        })
    }
}

pub struct Workload {
    pub property: Property,
    field_name_list: Vec<String>,
    field_chooser: Box<dyn Fn() -> usize + Send + Sync>,
    field_length_generator: Box<dyn Fn() -> usize + Send + Sync>,
    key_sequence: Box<dyn Fn() -> u64 + Send + Sync>,
    transaction_insert_key_sequence: Arc<AcknowledgedCounterGenerator>,
    key_chooser: Box<dyn Fn() -> u64 + Send + Sync>,
    operation_chooser: Box<dyn Fn() -> OpKind + Send + Sync>,
    scan_length: Box<dyn Fn() -> usize + Send + Sync>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpKind {
    Read(Opaque),
    Update(Opaque),
    Insert(Opaque),
    Scan(Opaque),
    ReadModifyWrite(Opaque, Opaque),
}

struct AcknowledgedCounterGenerator {
    counter: AtomicU64,
    limit: AtomicU64,
    window: Mutex<[bool; Self::WINDOW_SIZE]>,
}
impl AcknowledgedCounterGenerator {
    const WINDOW_SIZE: usize = 1 << 20;
    const WINDOW_MASK: usize = Self::WINDOW_SIZE - 1;
    fn new(start: u64) -> Self {
        Self {
            counter: AtomicU64::new(start),
            limit: AtomicU64::new(start),
            window: Mutex::new([false; Self::WINDOW_SIZE]),
        }
    }

    fn next_value(&self) -> u64 {
        self.counter.fetch_add(1, Ordering::SeqCst)
    }

    fn last_value(&self) -> u64 {
        self.limit.load(Ordering::SeqCst)
    }

    fn acknowledge(&self, value: u64) {
        let mut window = self.window.lock().unwrap();
        assert!(!window[value as usize & Self::WINDOW_MASK]);
        window[value as usize & Self::WINDOW_MASK] = true;

        let mut out_index = 0;
        for index in self.limit.load(Ordering::SeqCst).. {
            if !window[index as usize & Self::WINDOW_MASK] {
                out_index = index;
                break;
            }
            window[index as usize & Self::WINDOW_MASK] = false;
        }
        assert_ne!(out_index, 0);
        self.limit.store(out_index, Ordering::SeqCst);
    }
}

// there is a Zipf distribution from rand_distr, the reason not to use:
// * not sure how to understand the relationship of the factors between it and
//   YCSB version
// * it has no support to adjust item count, and closure cannot be the interface
//   anyway
struct ZipfianGenerator {
    item_count: u64,
    base: u64,
    alpha: f64,
    zetan: f64,
    eta: f64,
    theta: f64,
    zeta2theta: f64,
    count_for_zeta: u64,
    allow_item_count_decrease: bool,
    last_value: u64,
}

impl ZipfianGenerator {
    const SCRAMBLED_ITEM_COUNT: u64 = 10000000000;
    fn new(min: u64, max: u64) -> Self {
        let item_count = max - min;
        let zipfian_constant = 0.99;
        let theta = zipfian_constant;
        let zeta2theta = Self::zeta_static(0, 2, theta, 0.0);
        let zetan = if item_count == Self::SCRAMBLED_ITEM_COUNT {
            26.46902820178302
        } else {
            Self::zeta_static(0, item_count, zipfian_constant, 0.0)
        };
        let mut s = Self {
            item_count,
            base: min,
            theta,
            zeta2theta,
            alpha: 1.0 / (1.0 - theta),
            zetan,
            count_for_zeta: item_count,
            eta: (1.0 - (2.0 / item_count as f64).powf(1.0 - theta)) / (1.0 - zeta2theta / zetan),
            allow_item_count_decrease: false,
            last_value: Default::default(),
        };
        s.next_value();
        s
    }

    fn zeta_static(st: u64, n: u64, theta: f64, initial_sum: f64) -> f64 {
        let mut sum = initial_sum;
        for i in st..n {
            if n / 1000 != 0 && i % (n / 1000) == 0 {
                debug!("zeta static: {}/{}", i, n);
            }
            sum += 1.0 / ((i + 1) as f64).powf(theta);
        }
        sum
    }

    fn next_value(&mut self) -> u64 {
        self.next_long(self.item_count)
    }

    fn next_long(&mut self, item_count: u64) -> u64 {
        if item_count != self.count_for_zeta {
            // YCSB's zeta method is too OOP so I use zeta_static instead
            if item_count > self.count_for_zeta {
                self.zetan =
                    Self::zeta_static(self.count_for_zeta, item_count, self.theta, self.zetan);
            } else if self.allow_item_count_decrease {
                self.zetan = Self::zeta_static(0, item_count, self.theta, 0.0);
            }
            self.count_for_zeta = item_count;
            self.eta = (1.0 - (2.0 / self.item_count as f64).powf(1.0 - self.theta))
                / (1.0 - self.zeta2theta / self.zetan);
        }
        let u: f64 = random();
        let uz = u * self.zetan;
        if uz < 1.0 {
            self.base
        } else if uz < 1.0 + 0.5_f64.powf(self.theta) {
            self.base + 1
        } else {
            let ret = self.base
                + (self.item_count * (self.eta * u - self.eta + 1.0).powf(self.alpha) as u64);
            self.last_value = ret;
            ret
        }
    }
}

impl Workload {
    fn uniform_generator<U: SampleUniform + Clone + Send + Sync>(
        low: U,
        high: U,
    ) -> impl Fn() -> U + Send + Sync {
        move || thread_rng().sample(Uniform::new(low.clone(), high.clone()))
    }

    fn counter_generator(start: u64) -> impl Fn() -> u64 + Send + Sync {
        let counter = AtomicU64::new(start);
        move || counter.fetch_add(1, Ordering::SeqCst)
    }

    fn scrambled_zipfian_generator(min: u64, max: u64) -> impl Fn() -> u64 + Send + Sync {
        let zipfian = Mutex::new(ZipfianGenerator::new(
            0,
            ZipfianGenerator::SCRAMBLED_ITEM_COUNT,
        ));
        move || min + Self::fnvhash64(zipfian.lock().unwrap().next_value()) % (max - min)
    }

    fn skewed_latest_generator(
        basis: Arc<AcknowledgedCounterGenerator>,
    ) -> impl Fn() -> u64 + Send + Sync {
        let zipfian = Mutex::new(ZipfianGenerator::new(0, basis.last_value()));
        move || {
            let max = basis.last_value();
            max - zipfian.lock().unwrap().next_long(max) - 1
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

    fn get_field_length_generator(property: &Property) -> impl Fn() -> usize + Send + Sync {
        let field_length = property.field_length;
        // TODO other distributions
        move || field_length
    }

    fn create_operation_generator(property: &Property) -> impl Fn() -> OpKind + Send + Sync {
        let mut low = 0.0;
        let mut predicate_list = Vec::new();
        let mut add_value = move |predicate_list: &mut Vec<_>, proportion, value: OpKind| {
            predicate_list.push(Box::new(move |p| {
                if (low..low + proportion).contains(&p) {
                    Some(value.clone())
                } else {
                    None
                }
            }));
            low += proportion;
        };

        if property.read_proportion > 0.0 {
            add_value(
                &mut predicate_list,
                property.read_proportion,
                OpKind::Read(Opaque::default()),
            );
        }
        if property.update_proportion > 0.0 {
            add_value(
                &mut predicate_list,
                property.update_proportion,
                OpKind::Update(Opaque::default()),
            );
        }
        if property.insert_proportion > 0.0 {
            add_value(
                &mut predicate_list,
                property.insert_proportion,
                OpKind::Insert(Opaque::default()),
            );
        }
        if property.scan_proportion > 0.0 {
            add_value(
                &mut predicate_list,
                property.scan_proportion,
                OpKind::Scan(Opaque::default()),
            );
        }
        if property.read_modify_write_proportion > 0.0 {
            add_value(
                &mut predicate_list,
                property.read_modify_write_proportion,
                OpKind::ReadModifyWrite(Opaque::default(), Opaque::default()),
            );
        }

        move || {
            let p = random();
            for predicate in &predicate_list {
                if let Some(op_kind) = predicate(p) {
                    return op_kind;
                }
            }
            unreachable!()
        }
    }

    fn build_single_value(&self, key: &str) -> HashMap<String, Opaque> {
        let mut value_table = HashMap::new();
        let field = (self.field_chooser)();
        let field = self.field_name_list[field].clone();
        let value = if self.property.data_integrity {
            self.build_deterministic_value(key, &field)
        } else {
            let field_length = (self.field_length_generator)();
            let mut rng = thread_rng();
            repeat_with(|| rng.gen()).take(field_length).collect()
        };
        value_table.insert(field, value);
        value_table
    }

    fn build_value_table(&self, key: &str) -> HashMap<String, Opaque> {
        let mut value_table = HashMap::new();
        for field in self.field_name_list.clone() {
            let value = if self.property.data_integrity {
                self.build_deterministic_value(key, &field)
            } else {
                let field_length = (self.field_length_generator)();
                let mut rng = thread_rng();
                repeat_with(|| rng.gen()).take(field_length).collect()
            };
            value_table.insert(field, value);
        }
        value_table
    }

    fn build_deterministic_value(&self, key: &str, field: &str) -> Opaque {
        let field_length = (self.field_length_generator)();
        let mut value = format!("{key}:{field}").as_bytes().to_vec();
        while value.len() < field_length {
            value.extend(b":");
            value.extend(
                {
                    let mut hasher = DefaultHasher::new();
                    value.hash(&mut hasher);
                    hasher.finish()
                }
                .to_string()
                .as_bytes(),
            )
        }
        value.truncate(field_length);
        value
    }

    pub fn new(mut property: Property) -> Self {
        assert_ne!(property.record_count, 0);
        if property.do_transaction {
            // i didn't read this, but infer it from usage
            assert_eq!(property.insert_start, 0);
            assert_eq!(property.insert_count, 0);
        }
        if property.insert_count == 0 {
            property.insert_count = property.record_count - property.insert_start;
        }

        let transaction_insert_key_sequence =
            Arc::new(AcknowledgedCounterGenerator::new(property.record_count));
        let key_chooser: Box<dyn Fn() -> _ + Send + Sync> = match property.request_distribution {
            Distribution::Uniform => Box::new(Self::uniform_generator(
                property.insert_start,
                property.insert_start + property.insert_count,
            )),
            Distribution::Zipfian => {
                let expected_new_key =
                    (property.operation_count as f32 * property.insert_proportion * 2.0) as u64;
                Box::new(Self::scrambled_zipfian_generator(
                    property.insert_start,
                    property.insert_start + property.insert_count + expected_new_key,
                ))
            }
            Distribution::Latest => Box::new(Self::skewed_latest_generator(
                transaction_insert_key_sequence.clone(),
            )),
            _ => todo!(),
        };
        let scan_length: Box<dyn Fn() -> _ + Send + Sync> = match property.scan_length_distribution
        {
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
            transaction_insert_key_sequence,
            key_chooser,
            operation_chooser: Box::new(Self::create_operation_generator(&property)),
            scan_length,
            property,
        }
    }

    fn next_key_number(&self) -> u64 {
        let mut last_value;
        let mut next_value;
        while {
            last_value = self.transaction_insert_key_sequence.last_value();
            next_value = (self.key_chooser)();
            last_value < next_value
        } {}
        if self.property.request_distribution == Distribution::Exponential {
            last_value - next_value
        } else {
            next_value
        }
    }

    pub fn one_op(&self) -> (OpKind, Box<dyn FnOnce(&Self) + Send>) {
        if !self.property.do_transaction {
            let key_number = (self.key_sequence)();
            assert!(key_number < self.property.insert_start + self.property.insert_count);
            let key = Self::build_key_name(
                key_number,
                self.property.zero_padding,
                self.property.insert_order,
            );
            let value_table = self.build_value_table(&key);
            let mut buffer = Opaque::new();
            serialize(Op::Insert(self.property.table.clone(), key, value_table))(&mut buffer);
            return (OpKind::Insert(buffer), Box::new(|_| {}));
        }

        let mut op_kind = (self.operation_chooser)();
        let key_number = if !matches!(op_kind, OpKind::Insert(_)) {
            self.next_key_number()
        } else {
            self.transaction_insert_key_sequence.next_value()
        };
        let key = Self::build_key_name(
            key_number,
            self.property.zero_padding,
            self.property.insert_order,
        );
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

        let mut post_action: Box<dyn FnOnce(&Self) + Send> = Box::new(|_| {});
        match &mut op_kind {
            OpKind::Read(op) => {
                serialize(Op::Read(table, key, field_set))(op);
                // TODO data integrity
            }
            OpKind::ReadModifyWrite(read_op, update_op) => {
                serialize(Op::Read(table.clone(), key.clone(), field_set))(read_op);
                serialize(Op::Update(table, key, value_table))(update_op);
            }
            OpKind::Scan(op) => {
                let len = (self.scan_length)();
                serialize(Op::Scan(table, key, len, field_set))(op);
            }
            OpKind::Update(op) => {
                serialize(Op::Update(table, key, value_table))(op);
            }
            OpKind::Insert(op) => {
                serialize(Op::Insert(table, key, value_table))(op);
                post_action = Box::new(move |context| {
                    context
                        .transaction_insert_key_sequence
                        .acknowledge(key_number)
                });
            }
        }
        (op_kind, post_action)
    }
}
