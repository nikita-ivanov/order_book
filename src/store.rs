mod csv;
use anyhow::Result;
mod parquet;

use std::collections::HashMap;

use crate::{book::snapshot::OrderBookSnapshot, store::parquet::ParquetSink};

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

pub fn construct_file_stores(
    symbols: &Vec<String>,
    depth: usize,
) -> HashMap<String, Box<dyn OrderBookSink>> {
    let mut stores = HashMap::<String, Box<dyn OrderBookSink>>::new();
    for symbol in symbols {
        stores.insert(
            String::from(symbol),
            Box::new(ParquetSink::new(symbol.to_string(), depth, None)),
        );
    }

    stores
}
