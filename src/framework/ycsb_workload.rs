pub struct Property {
    pub field_count: usize,
    pub field_length: u32,
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
    pub insert_start: usize,
    pub insert_count: usize,
    pub zero_padding: usize,
    pub insert_order: Order,
    pub field_name_prefix: String,
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
        }
    }
}

pub enum Distribution {
    Uniform,
    Zipfian,
    HotSpot,
    Sequential,
    Exponential,
    Latest,
}

pub enum Order {
    Ordered,
    Hashed,
}
