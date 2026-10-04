mod csv;
mod parquet;

use std::collections::HashMap;

use crate::{book::order_book::OrderBook, store::parquet::ParquetStore};
use csv::CsvStore;

#[derive(Debug)]
pub enum OrderBookStore {
    CsvOrderBookStore(CsvStore),
    ParquetOrderBookStore(ParquetStore),
}

impl OrderBookStore {
    pub fn save(&mut self, ob: &OrderBook) {
        match self {
            OrderBookStore::CsvOrderBookStore(store) => {
                store.save(ob);
            }
            OrderBookStore::ParquetOrderBookStore(store) => {
                store.save(ob).unwrap();
            }
        }
    }
}

pub fn construct_file_stores(
    symbols: &Vec<String>,
    depth: usize,
) -> HashMap<String, OrderBookStore> {
    let mut stores = HashMap::<String, OrderBookStore>::new();
    for symbol in symbols {
        // let symbol_slugified = &String::from(symbol).replace("/", "_").to_lowercase();

        // let path = dirs::home_dir()
        //     .expect("Could not infer home directory")
        //     // .join(format!("ob_data/{symbol_slugified}.csv"));
        //     .join(format!("ob_data/{symbol_slugified}.parquet"));

        // let prefix = path.parent().expect("Parent is home dir / ob_data");
        // fs::create_dir_all(prefix).unwrap_or_else(|_| panic!("Failed to create {prefix:?}"));

        stores.insert(
            String::from(symbol),
            // OrderBookStore::CsvOrderBookStore(CsvStore::new(path.as_path(), depth)),
            OrderBookStore::ParquetOrderBookStore(ParquetStore::new(
                symbol.to_string(),
                depth,
                None,
            )),
        );
    }

    stores
}
