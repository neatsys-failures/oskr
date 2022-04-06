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
