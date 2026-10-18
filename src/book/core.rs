/// Helper to convert integer order book data to f64
pub fn int_to_float(number: i64, precision: usize) -> f64 {
    (number as f64) / (10_u32.pow(precision as u32) as f64)
}
