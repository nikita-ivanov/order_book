use anyhow::{Result, anyhow};
use arrow_array::{ArrayRef, Float64Array, Int64Array, RecordBatch};
use arrow_schema::{DataType, Field, Schema};
use chrono::Utc;
use parquet::arrow::ArrowWriter;
use parquet::file::properties::WriterProperties;
use std::fs;
use std::fs::{File, OpenOptions};
use std::path::PathBuf;
use std::sync::Arc;

use crate::book::order_book::OrderBook;
use crate::book::order_book::int_to_float;

pub struct ParquetStore {
    writer: Option<ArrowWriter<File>>,
    schema: Arc<Schema>,
}

impl ParquetStore {
    pub fn new(symbol: String, depth: usize, file_path: Option<PathBuf>) -> ParquetStore {
        let file_path = match file_path {
            Some(path) => path,
            None => {
                let symbol_slug = symbol.replace("/", "_").to_lowercase();
                let file_name = uuid::Uuid::new_v4().to_string();
                dirs::home_dir()
                    .expect("Could not infer home directory")
                    .join(format!("{symbol_slug}/{file_name}.parquet"))
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

        let mut fields = vec![Field::new("timestamp_ns", DataType::Int64, false)];

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

        ParquetStore {
            writer: Some(writer),
            schema,
        }
    }

    pub fn save(&mut self, ob: &OrderBook) -> Result<()> {
        let now = Utc::now().timestamp_nanos_opt().unwrap();
        let timestamp = Int64Array::from(vec![now]);

        let mut columns: Vec<ArrayRef> = vec![Arc::new(timestamp)];

        let mut i = 0;
        for (p, q) in ob.bids.iter().rev() {
            let price = int_to_float(*p, ob.pair_decimals);
            let quantity = int_to_float(*q, ob.lot_decimals);

            let price_col = Float64Array::from(vec![price]);
            columns.push(Arc::new(price_col));

            let quantity_col = Float64Array::from(vec![quantity]);
            columns.push(Arc::new(quantity_col));

            i += 1;
        }

        // pad to make sure we guarantee number of cols
        if i < ob.depth {
            for _ in i..ob.depth {
                let price_col = Float64Array::from(vec![0.0]);
                columns.push(Arc::new(price_col));

                let quantity_col = Float64Array::from(vec![0.0]);
                columns.push(Arc::new(quantity_col));
            }
        }

        let mut i = 0;
        for (p, q) in ob.asks.iter() {
            let price = int_to_float(*p, ob.pair_decimals);
            let quantity = int_to_float(*q, ob.lot_decimals);

            let price_col = Float64Array::from(vec![price]);
            columns.push(Arc::new(price_col));

            let quantity_col = Float64Array::from(vec![quantity]);
            columns.push(Arc::new(quantity_col));

            i += 1;
        }

        // pad to make sure we guarantee number of cols
        if i < ob.depth {
            for _ in i..ob.depth {
                let price_col = Float64Array::from(vec![0.0]);
                columns.push(Arc::new(price_col));

                let quantity_col = Float64Array::from(vec![0.0]);
                columns.push(Arc::new(quantity_col));
            }
        }

        let record = RecordBatch::try_new(Arc::clone(&self.schema), columns).unwrap();

        match self.writer.as_mut() {
            Some(writer) => Ok(writer.write(&record)?),
            None => Err(anyhow!("writer already closed")),
        }
    }

    pub fn finish(&mut self) -> Result<()> {
        match self.writer.take() {
            Some(mut writer) => match writer.finish() {
                Ok(_) => Ok(()),
                Err(e) => Err(anyhow!("failed to finish the writer, error was: {e:?}")),
            },
            None => Ok(()),
        }
    }
}

impl Drop for ParquetStore {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use arrow_array::Float64Array;
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

    use crate::{
        book::order_book::{BookUpdate, Level, OrderBook},
        store::parquet::ParquetStore,
    };

    #[test]
    fn test_parquet_store() {
        let depth = 3;
        let path = tempfile::NamedTempFile::new().unwrap().path().to_path_buf();
        let mut store = ParquetStore::new("ETH/USD".to_string(), depth, Some(path.clone()));

        let mut ob = OrderBook::new("dummy".to_string(), depth as u16, 0, 0);

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
        })
        .unwrap();

        store.save(&ob).unwrap();

        store.finish().unwrap();

        let file = File::open(&path).unwrap();
        let mut reader = ParquetRecordBatchReaderBuilder::try_new(file)
            .unwrap()
            .build()
            .unwrap();

        let value = reader.next().unwrap().unwrap();

        // depth * 4 + 1
        assert_eq!(value.num_columns(), 13);
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
    }
}
