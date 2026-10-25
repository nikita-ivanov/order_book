use anyhow::{Result, anyhow};
use arrow_array::{ArrayRef, Float64Array, Int64Array, RecordBatch};
use arrow_schema::{DataType, Field, Schema};
use chrono::{DateTime, Utc};
use parquet::arrow::ArrowWriter;
use parquet::file::properties::WriterProperties;
use std::collections::HashMap;
use std::fs;
use std::fs::{File, OpenOptions};
use std::ops::Index;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::book::snapshot::OrderBookSnapshot;
use crate::store::OrderBookSink;

#[derive(Debug)]
pub struct ParquetSink {
    writer: Option<ArrowWriter<File>>,
    schema: Arc<Schema>,
    buffer_size: usize,
    buffer: BufferArray,
}

#[derive(Debug)]
struct BufferArray {
    bids: HashMap<u16, Vec<Option<(f64, f64)>>>,
    asks: HashMap<u16, Vec<Option<(f64, f64)>>>,
    published: Vec<i64>,
    recorded: Vec<i64>,
    size: usize,
}

impl BufferArray {
    fn new() -> BufferArray {
        BufferArray {
            bids: HashMap::new(),
            asks: HashMap::new(),
            published: Vec::new(),
            recorded: Vec::new(),
            size: 0,
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
                .expect("level should be there")
                .push(bid);

            self.asks
                .get_mut(&level)
                .expect("level should be there")
                .push(ask);
        }

        self.published.push(
            ob.timestamp
                .timestamp_nanos_opt()
                .expect("invalid datetime"),
        );

        self.recorded
            .push(Utc::now().timestamp_nanos_opt().expect("invalid datetime"));

        self.size += 1;
    }

    fn empty(&mut self) {
        self.bids = HashMap::new();
        self.asks = HashMap::new();
        self.published = Vec::new();
        self.recorded = Vec::new();
    }
}

impl ParquetSink {
    pub fn new(
        symbol: String,
        depth: usize,
        buffer_size: usize,
        file_path: Option<PathBuf>,
    ) -> ParquetSink {
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
                    .join(format!("ob_data/{symbol_slug}/{file_name}.parquet"))
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

        ParquetSink {
            writer: Some(writer),
            schema,
            buffer_size,
            buffer: BufferArray::new(),
        }
    }
}

impl Drop for ParquetSink {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

impl OrderBookSink for ParquetSink {
    fn write(&mut self, ob: &OrderBookSnapshot) -> Result<()> {
        self.buffer.apply(ob);

        if self.buffer.size >= self.buffer_size {
            let mut columns: Vec<ArrayRef> = Vec::new();

            let recorded_array = self.buffer.recorded.drain(..).collect::<Vec<i64>>();
            columns.push(Arc::new(Int64Array::from(recorded_array)));

            let published_array = self.buffer.published.drain(..).collect::<Vec<i64>>();
            columns.push(Arc::new(Int64Array::from(published_array)));

            for level in 1..ob.depth + 1 {
                let bids: Vec<Option<(f64, f64)>> = self
                    .buffer
                    .bids
                    .get_mut(&level)
                    .expect("level should be there")
                    .drain(..)
                    .collect();

                let bid_prices: Vec<Option<f64>> =
                    bids.iter().map(|opt| opt.map(|x| x.0)).collect();

                let bid_quantities: Vec<Option<f64>> =
                    bids.iter().map(|opt| opt.map(|x| x.1)).collect();

                columns.push(Arc::new(Float64Array::from(bid_prices)));
                columns.push(Arc::new(Float64Array::from(bid_quantities)));

                let asks: Vec<Option<(f64, f64)>> = self
                    .buffer
                    .asks
                    .get_mut(&level)
                    .expect("level should be there")
                    .drain(..)
                    .collect();

                let ask_prices: Vec<Option<f64>> =
                    asks.iter().map(|opt| opt.map(|x| x.0)).collect();

                let ask_quantities: Vec<Option<f64>> =
                    asks.iter().map(|opt| opt.map(|x| x.1)).collect();

                columns.push(Arc::new(Float64Array::from(ask_prices)));
                columns.push(Arc::new(Float64Array::from(ask_quantities)));
            }

            let record = RecordBatch::try_new(Arc::clone(&self.schema), columns).unwrap();

            match self.writer.as_mut() {
                Some(writer) => Ok(writer.write(&record)?),
                None => Err(anyhow!("writer already closed")),
            };
        }

        Ok(())

        // let timestamp = Int64Array::from(vec![now]);
        // let published = Int64Array::from(vec![ob.timestamp.timestamp_nanos_opt()]);
        //
        // let mut columns: Vec<ArrayRef> = vec![Arc::new(timestamp), Arc::new(published)];
        //
        // let mut i = 0;
        // for (p, q) in ob.get_bids().iter() {
        //     let price_col = Float64Array::from(vec![*p]);
        //     columns.push(Arc::new(price_col));
        //
        //     let quantity_col = Float64Array::from(vec![*q]);
        //     columns.push(Arc::new(quantity_col));
        //
        //     i += 1;
        // }
        //
        // // pad to make sure we guarantee number of cols
        // if i < ob.depth {
        //     for _ in i..ob.depth {
        //         let price_col = Float64Array::from(vec![0.0]);
        //         columns.push(Arc::new(price_col));
        //
        //         let quantity_col = Float64Array::from(vec![0.0]);
        //         columns.push(Arc::new(quantity_col));
        //     }
        // }
        //
        // let mut i = 0;
        // for (p, q) in ob.get_asks().iter() {
        //     let price_col = Float64Array::from(vec![*p]);
        //     columns.push(Arc::new(price_col));
        //
        //     let quantity_col = Float64Array::from(vec![*q]);
        //     columns.push(Arc::new(quantity_col));
        //
        //     i += 1;
        // }
        //
        // // pad to make sure we guarantee number of cols
        // if i < ob.depth {
        //     for _ in i..ob.depth {
        //         let price_col = Float64Array::from(vec![0.0]);
        //         columns.push(Arc::new(price_col));
        //
        //         let quantity_col = Float64Array::from(vec![0.0]);
        //         columns.push(Arc::new(quantity_col));
        //     }
        // }
        //
        // let record = RecordBatch::try_new(Arc::clone(&self.schema), columns).unwrap();
        //
        // match self.writer.as_mut() {
        //     Some(writer) => Ok(writer.write(&record)?),
        //     None => Err(anyhow!("writer already closed")),
        // }
    }

    fn finish(&mut self) -> Result<()> {
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
        store::{OrderBookSink, parquet::ParquetSink},
    };

    #[test]
    fn test_parquet_store() {
        let depth = 3;
        let path = tempfile::NamedTempFile::new().unwrap().path().to_path_buf();
        let mut store = ParquetSink::new("ETH/USD".to_string(), depth, Some(path.clone()));

        let mut ob = OrderBook::new("dummy".to_string(), depth as u16, 0, 0, Utc::now());

        ob.apply_update(BookUpdate {
            bids: Some(vec![
                Level {
                    price: 12,
                    quantity: 7,
                },
                Level {
                    price: 13,
                    quantity: 1,
                },
                Level {
                    price: 15,
                    quantity: 3,
                },
            ]),

            asks: Some(vec![
                Level {
                    price: 15,
                    quantity: 7,
                },
                Level {
                    price: 14,
                    quantity: 1,
                },
                Level {
                    price: 20,
                    quantity: 3,
                },
            ]),
            timestamp: Utc::now(),
        })
        .unwrap();

        let now = Utc::now().timestamp_nanos_opt().unwrap();

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
        assert_eq!(value.num_rows(), 1);

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

        dbg!(published_ns);

        assert!(published_ns >= now - 1000 * 1000);
        assert!(published_ns < now + 1000 * 1000);
    }
}
