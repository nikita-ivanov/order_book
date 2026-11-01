mod csv;
use anyhow::Result;
mod parquet;
mod parquet_long;
pub mod writer;

use crate::{
    book::snapshot::OrderBookSnapshot,
    store::{parquet::ParquetWideSink, parquet_long::ParquetLongSink},
};

pub trait OrderBookSink {
    fn write(&mut self, ob: &OrderBookSnapshot) -> Result<()>;
    fn finish(&mut self) -> Result<()>;
}

pub struct NoOpSink;
impl OrderBookSink for NoOpSink {
    fn write(&mut self, _ob: &OrderBookSnapshot) -> Result<()> {
        Ok(())
    }

    fn finish(&mut self) -> Result<()> {
        Ok(())
    }
}

pub fn construct_file_store(symbol: &str, depth: usize) -> Box<dyn OrderBookSink + Send> {
    // Box::new(ParquetSink::new(symbol.to_string(), depth, 20, None))
    Box::new(ParquetLongSink::new(symbol.to_string(), 1000, None))
}
