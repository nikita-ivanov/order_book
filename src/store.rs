mod csv;
use anyhow::Result;
use clap::ValueEnum;
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

#[derive(Debug, Clone, ValueEnum)]
pub enum OrderBookStoreKind {
    NoOps,
    ParquetWide,
    ParquetLong,
}

impl ToString for OrderBookStoreKind {
    fn to_string(&self) -> String {
        match self {
            OrderBookStoreKind::NoOps => "no-ops".to_string(),
            OrderBookStoreKind::ParquetLong => "parquet-long".to_string(),
            OrderBookStoreKind::ParquetWide => "parquet-wide".to_string(),
        }
    }
}

pub fn construct_file_store(
    symbol: &str,
    depth: usize,
    kind: OrderBookStoreKind,
) -> Box<dyn OrderBookSink + Send> {
    match kind {
        OrderBookStoreKind::NoOps => Box::new(NoOpSink {}),
        OrderBookStoreKind::ParquetLong => Box::new(ParquetLongSink::new(symbol, 1_000, None)),
        OrderBookStoreKind::ParquetWide => {
            Box::new(ParquetWideSink::new(symbol, depth, 1_000, None))
        }
    }
}
