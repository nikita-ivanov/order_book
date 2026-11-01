use anyhow::{Result, anyhow};
use arrow_array::{ArrayRef, Float64Array, Int64Array, RecordBatch};
use arrow_schema::{DataType, Field, Schema};
use chrono::Utc;
use parquet::arrow::ArrowWriter;
use parquet::file::properties::WriterProperties;
use std::collections::HashMap;
use std::fs;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::book::snapshot::OrderBookSnapshot;
use crate::store::OrderBookSink;

#[derive(Debug)]
pub struct ParquetWideSink {
    writer: Option<ArrowWriter<File>>,
    schema: Arc<Schema>,
    buffer_size: usize,
    depth: usize,
    buffer: BufferArray,
}

#[derive(Debug)]
struct BufferArray {
    // TODO: could be vec of vec to avoid hashing
    bids: HashMap<u16, Vec<Option<(f64, f64)>>>,
    asks: HashMap<u16, Vec<Option<(f64, f64)>>>,
    published: Vec<i64>,
    recorded: Vec<i64>,
    current_size: usize,
}

impl BufferArray {
    fn new(depth: usize, buffer_limit: usize) -> BufferArray {
        let mut bids: HashMap<u16, Vec<Option<(f64, f64)>>> = HashMap::new();
        let mut asks: HashMap<u16, Vec<Option<(f64, f64)>>> = HashMap::new();

        for level in 1..depth + 1 {
            bids.insert(level as u16, Vec::with_capacity(buffer_limit));
            asks.insert(level as u16, Vec::with_capacity(buffer_limit));
        }

        BufferArray {
            bids,
            asks,
            published: Vec::with_capacity(buffer_limit),
            recorded: Vec::with_capacity(buffer_limit),
            current_size: 0,
        }
    }

    fn apply(&mut self, ob: &OrderBookSnapshot) {
        let mut bids: Vec<Option<(f64, f64)>> =
            ob.get_bids().iter().map(|pair| Some(*pair)).collect();

        let mut asks: Vec<Option<(f64, f64)>> =
            ob.get_asks().iter().map(|pair| Some(*pair)).collect();

        // handle potential cases when bids and asks in order book have different lengths
        while bids.len() < ob.depth as usize {
            bids.push(None);
        }

        while asks.len() < ob.depth as usize {
            asks.push(None);
        }

        for level in 1..ob.depth + 1 {
            let idx = (level - 1) as usize;
            let bid = bids[idx];
            let ask = asks[idx];

            self.bids
                .get_mut(&level)
                .expect(&format!("missing level {level} in bids"))
                .push(bid);

            self.asks
                .get_mut(&level)
                .expect(&format!("missing level {level} in asks"))
                .push(ask);
        }

        self.published.push(
            ob.timestamp
                .timestamp_nanos_opt()
                .expect("invalid datetime"),
        );

        self.recorded
            .push(Utc::now().timestamp_nanos_opt().expect("invalid datetime"));

        self.current_size += 1;
    }

    fn empty(&mut self) {
        for v in self.bids.values_mut() {
            v.clear();
        }

        for v in self.asks.values_mut() {
            v.clear();
        }

        self.published.clear();
        self.recorded.clear();

        self.current_size = 0;
    }
}

// TODO: make fields nullable where we may inserts nulls (bids and asks)
impl ParquetWideSink {
    pub fn new(
        symbol: String,
        depth: usize,
        buffer_size: usize,
        file_path: Option<PathBuf>,
    ) -> ParquetWideSink {
        let file_path = match file_path {
            Some(path) => {
                if Path::exists(&path) {
                    println!("Supplied path exists, contents will be overwritten: {path:?}");
                }
                path
            }
            None => {
                let symbol_slug = symbol.replace("/", "_").to_lowercase();
                let file_name = uuid::Uuid::new_v4().to_string();
                dirs::home_dir()
                    .expect("Could not infer home directory")
                    .join(format!(
                        "ob_data/symbol={symbol_slug}/depth={depth}/{file_name}.parquet"
                    ))
            }
        };

        let prefix = file_path.parent().expect("parent dir should be available");
        fs::create_dir_all(prefix).unwrap_or_else(|_| panic!("Failed to create {prefix:?}"));

        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&file_path)
            .unwrap();

        let mut fields = vec![
            Field::new("inserted_ns", DataType::Int64, false),
            Field::new("published_ns", DataType::Int64, true),
        ];

        for i in 1..depth + 1 {
            fields.push(Field::new(format!("bid_p_{i}"), DataType::Float64, false));
            fields.push(Field::new(format!("bid_q_{i}"), DataType::Float64, false));
        }

        for i in 1..depth + 1 {
            fields.push(Field::new(format!("ask_p_{i}"), DataType::Float64, false));
            fields.push(Field::new(format!("ask_q_{i}"), DataType::Float64, false));
        }

        let schema = Arc::new(Schema::new(fields));

        let props = WriterProperties::builder()
            .set_max_row_group_row_count(Some(10_000))
            .build();

        let writer = ArrowWriter::try_new(file, schema.clone(), Some(props)).unwrap();

        ParquetWideSink {
            writer: Some(writer),
            schema,
            buffer_size,
            depth,
            buffer: BufferArray::new(depth, buffer_size),
        }
    }

    fn flush(&mut self) -> Result<()> {
        println!("Flushing ParquetSink");
        let mut columns: Vec<ArrayRef> = Vec::new();

        let recorded_array = std::mem::replace(
            &mut self.buffer.recorded,
            Vec::with_capacity(self.buffer_size),
        );
        columns.push(Arc::new(Int64Array::from(recorded_array)));

        let published_array = std::mem::replace(
            &mut self.buffer.published,
            Vec::with_capacity(self.buffer_size),
        );
        columns.push(Arc::new(Int64Array::from(published_array)));

        for level in 1..self.depth + 1 {
            let bids: Vec<Option<(f64, f64)>> = std::mem::replace(
                self.buffer
                    .bids
                    .get_mut(&(level as u16))
                    .expect("level should be there"),
                Vec::with_capacity(self.buffer_size),
            );

            let bid_prices: Vec<Option<f64>> = bids.iter().map(|opt| opt.map(|x| x.0)).collect();

            let bid_quantities: Vec<Option<f64>> =
                bids.iter().map(|opt| opt.map(|x| x.1)).collect();

            columns.push(Arc::new(Float64Array::from(bid_prices)));
            columns.push(Arc::new(Float64Array::from(bid_quantities)));
        }

        for level in 1..self.depth + 1 {
            let asks: Vec<Option<(f64, f64)>> = std::mem::replace(
                self.buffer
                    .asks
                    .get_mut(&(level as u16))
                    .expect("level should be there"),
                Vec::with_capacity(self.buffer_size),
            );

            let ask_prices: Vec<Option<f64>> = asks.iter().map(|opt| opt.map(|x| x.0)).collect();

            let ask_quantities: Vec<Option<f64>> =
                asks.iter().map(|opt| opt.map(|x| x.1)).collect();

            columns.push(Arc::new(Float64Array::from(ask_prices)));
            columns.push(Arc::new(Float64Array::from(ask_quantities)));
        }

        let record = RecordBatch::try_new(Arc::clone(&self.schema), columns).unwrap();

        let res = match self.writer.as_mut() {
            Some(writer) => Ok(writer.write(&record)?),
            None => Err(anyhow!("writer already closed")),
        };

        self.buffer.empty();

        res
    }
}

impl Drop for ParquetWideSink {
    fn drop(&mut self) {
        println!("dropping parquet store");
        let _ = self.finish();
    }
}

impl OrderBookSink for ParquetWideSink {
    fn write(&mut self, ob: &OrderBookSnapshot) -> Result<()> {
        self.buffer.apply(ob);

        if self.buffer.current_size >= self.buffer_size {
            println!("buffer size limit reached, flushing...");
            self.flush()?;
        }

        Ok(())
    }

    fn finish(&mut self) -> Result<()> {
        println!("finishing parquet store");
        self.flush()?;

        match self.writer.take() {
            Some(mut writer) => match writer.finish() {
                Ok(_) => Ok(()),
                Err(e) => Err(e.into()),
            },
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use arrow_array::{Float64Array, Int64Array};
    use chrono::Utc;
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

    use crate::{
        book::order_book::{BookUpdate, Level, OrderBook},
        store::{OrderBookSink, parquet::ParquetWideSink},
    };

    fn get_book_updates() -> [BookUpdate; 2] {
        let update1 = BookUpdate {
            bids: Some(vec![
                Level {
                    price: 13,
                    quantity: 1,
                },
                Level {
                    price: 12,
                    quantity: 7,
                },
                Level {
                    price: 15,
                    quantity: 3,
                },
            ]),

            asks: Some(vec![
                Level {
                    price: 18,
                    quantity: 7,
                },
                Level {
                    price: 14,
                    quantity: 1,
                },
                Level {
                    price: 23,
                    quantity: 3,
                },
            ]),
            timestamp: Utc::now(),
        };

        let update2 = BookUpdate {
            bids: Some(vec![
                Level {
                    price: 14,
                    quantity: 2,
                },
                Level {
                    price: 15,
                    quantity: 6,
                },
                Level {
                    price: 16,
                    quantity: 4,
                },
            ]),

            asks: Some(vec![
                Level {
                    price: 17,
                    quantity: 3,
                },
                Level {
                    price: 18,
                    quantity: 9,
                },
                Level {
                    price: 20,
                    quantity: 8,
                },
            ]),
            timestamp: Utc::now(),
        };

        [update1, update2]
    }

    #[test]
    fn test_parquet_store() {
        let depth = 3;
        let path = tempfile::NamedTempFile::new().unwrap().path().to_path_buf();
        let mut store = ParquetWideSink::new("ETH/USD".to_string(), depth, 5, Some(path.clone()));

        let now = Utc::now().timestamp_nanos_opt().unwrap();

        let mut ob = OrderBook::new("dummy".to_string(), depth as u16, 0, 0, Utc::now());
        let updates = get_book_updates();

        ob.apply_update(&updates[0]).unwrap();
        store.write(&ob.get_snapshot()).unwrap();

        ob.apply_update(&updates[1]).unwrap();
        store.write(&ob.get_snapshot()).unwrap();

        store.finish().unwrap();

        let file = File::open(&path).unwrap();
        let mut reader = ParquetRecordBatchReaderBuilder::try_new(file)
            .unwrap()
            .build()
            .unwrap();

        let value = reader.next().unwrap().unwrap();

        // depth * 4 + 2
        assert_eq!(value.num_columns(), 14);
        assert_eq!(value.num_rows(), 2);

        assert_eq!(
            value
                .column_by_name("bid_p_1")
                .unwrap()
                .as_any()
                .downcast_ref::<Float64Array>()
                .unwrap()
                .value(0),
            15.0
        );

        assert_eq!(
            value
                .column_by_name("bid_q_1")
                .unwrap()
                .as_any()
                .downcast_ref::<Float64Array>()
                .unwrap()
                .value(0),
            3.0
        );

        assert_eq!(
            value
                .column_by_name("ask_p_1")
                .unwrap()
                .as_any()
                .downcast_ref::<Float64Array>()
                .unwrap()
                .value(0),
            14.0
        );

        assert_eq!(
            value
                .column_by_name("ask_q_1")
                .unwrap()
                .as_any()
                .downcast_ref::<Float64Array>()
                .unwrap()
                .value(0),
            1.0
        );

        // check that prices are sorted
        let mut bids_0 = Vec::new();
        let mut bids_1 = Vec::new();
        let mut asks_0 = Vec::new();
        let mut asks_1 = Vec::new();

        for level in 1..=depth {
            let bid_name = format!("bid_p_{level}");
            let ask_name = format!("ask_p_{level}");
            bids_0.push(
                value
                    .column_by_name(&bid_name)
                    .unwrap()
                    .as_any()
                    .downcast_ref::<Float64Array>()
                    .unwrap()
                    .value(0),
            );
            bids_1.push(
                value
                    .column_by_name(&bid_name)
                    .unwrap()
                    .as_any()
                    .downcast_ref::<Float64Array>()
                    .unwrap()
                    .value(1),
            );

            asks_0.push(
                value
                    .column_by_name(&ask_name)
                    .unwrap()
                    .as_any()
                    .downcast_ref::<Float64Array>()
                    .unwrap()
                    .value(0),
            );
            asks_1.push(
                value
                    .column_by_name(&ask_name)
                    .unwrap()
                    .as_any()
                    .downcast_ref::<Float64Array>()
                    .unwrap()
                    .value(1),
            );
        }

        assert!(bids_0.iter().rev().is_sorted());
        assert!(bids_1.iter().rev().is_sorted());
        assert!(asks_0.is_sorted());
        assert!(asks_1.is_sorted());

        // check that inserted_ns is within reasonable range from now (1 second)
        let inserted_ns = value
            .column_by_name("inserted_ns")
            .unwrap()
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap()
            .value(0);

        assert!(inserted_ns >= now);
        assert!(inserted_ns < now + 1000 * 1000);

        let published_ns = value
            .column_by_name("published_ns")
            .unwrap()
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap()
            .value(0);

        assert!(published_ns >= now - 1000 * 1000);
        assert!(published_ns < now + 1000 * 1000);
    }
}
