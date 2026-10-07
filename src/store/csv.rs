use std::{
    fs::{File, OpenOptions},
    io::{BufWriter, Write},
    path::Path,
};

use anyhow::Result;
use chrono::Utc;

use crate::{
    book::order_book::{OrderBook, int_to_float},
    store::OrderBookSink,
};

#[derive(Debug)]
pub struct CsvSink {
    file: File,
}

impl CsvSink {
    pub fn new(path: &Path, depth: usize) -> CsvSink {
        let file_exists = path.exists();

        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)
            .unwrap();

        if !file_exists {
            // initialize headers
            let mut s = String::new();
            s += "timestamp_ns,";

            for i in 1..depth + 1 {
                s += &format!("bid_p_{i},");
                s += &format!("bid_q_{i},");
            }

            for i in 1..depth + 1 {
                s += &format!("ask_p_{i},");
                s += &format!("ask_q_{i},");
            }

            s.pop();
            s += "\n";

            file.write_all(s.as_bytes()).unwrap();
        }

        CsvSink { file }
    }
}

impl OrderBookSink for CsvSink {
    /// Saves order book in wide format. Number of columns will be equal to order book depth * 4.
    /// First price, quatitity pairs from bids are written, then from asks.
    fn write(&mut self, ob: &OrderBook) -> Result<()> {
        let mut writer = BufWriter::new(&self.file);

        let mut s = String::new();

        // TODO this is not entirely correct as we record time when we wrote the data,
        // not when it became available on the exchange or delivered to us (though latter should be
        // close)
        s += &(Utc::now().timestamp_nanos_opt().unwrap().to_string() + ",");

        // write (p,q) starting from best bid
        let mut i = 0;
        for (p, q) in ob.bids.iter().rev() {
            let price = int_to_float(*p, ob.pair_decimals);
            let quantity = int_to_float(*q, ob.lot_decimals);

            s = s + &price.to_string() + ",";
            s = s + &quantity.to_string() + ",";
            i += 1;
        }

        // pad to make sure we guarantee number of cols
        if i < ob.depth {
            for _ in i..ob.depth {
                s += "0,0,"; // p and q
            }
        }

        // same for asks (starting best ask)
        let mut i = 0;
        for (p, q) in ob.asks.iter() {
            let price = int_to_float(*p, ob.pair_decimals);
            let quantity = int_to_float(*q, ob.lot_decimals);

            s = s + &price.to_string() + ",";
            s = s + &quantity.to_string() + ",";
            i += 1;
        }

        if i < ob.depth {
            for _ in i..ob.depth {
                s += "0,0,";
            }
        }

        s.pop(); // trailing comma
        s += "\n";

        let bytes = s.as_bytes();
        writer.write_all(bytes)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::{
        book::order_book::{BookUpdate, Level, OrderBook},
        store::{OrderBookSink, csv::CsvSink},
    };

    #[test]
    fn test_file_store_works() {
        let file = tempfile::NamedTempFile::new().unwrap();

        // delete file since we want to simulate writing from zero;
        // otherwise we would not write header; note this keeps the parent directory
        fs::remove_file(file.path()).unwrap();

        let depth = 10;
        let mut store = CsvSink::new(file.path(), depth);
        let contents = fs::read_to_string(file.path()).unwrap();

        // this checks that header has expected number of elements;
        // we expect depth (10) * 4 + 1 (for timestamp_ms column)
        let (first_row, _) = contents.split_once("\n").unwrap();
        assert_eq!(first_row.split(",").collect::<Vec<&str>>().len(), 41);

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

        dbg!(&ob);

        // TODO should propagate error
        store.write(&ob).unwrap();

        let contents = fs::read_to_string(file.path()).unwrap();
        // drop header
        let (_, contents) = contents.split_once("\n").unwrap();
        // drop timestamp_ms
        let (_, contents) = contents.split_once(",").unwrap();
        assert_eq!(
            contents,
            "15,3,13,1,12,7,0,0,0,0,0,0,0,0,0,0,0,0,0,0,14,1,15,7,20,3,0,0,0,0,0,0,0,0,0,0,0,0,0,0\n"
                .to_string()
        );

        ob.apply_update(BookUpdate {
            bids: Some(vec![
                Level {
                    price: 17,
                    quantity: 9,
                },
                Level {
                    price: 8,
                    quantity: 2,
                },
            ]),
            asks: Some(vec![
                Level {
                    price: 13,
                    quantity: 1,
                },
                Level {
                    price: 22,
                    quantity: 3,
                },
            ]),
        })
        .unwrap();

        dbg!(&ob);

        store.write(&ob).unwrap();

        let contents = fs::read_to_string(file.path()).unwrap();
        let (_, contents) = contents.split_once("\n").unwrap();

        // remove timestamp_ms from both rows and then concatenate
        let (first_row, second_row) = contents.split_once("\n").unwrap();
        let (_, first_row) = first_row.split_once(",").unwrap();
        let (_, second_row) = second_row.split_once(",").unwrap();

        let contents = first_row.to_string() + "\n" + second_row;

        assert_eq!(
            contents,
            ("15,3,13,1,12,7,0,0,0,0,0,0,0,0,0,0,0,0,0,0,14,1,15,7,20,3,0,0,0,0,0,0,0,0,0,0,0,0,0,0\n".to_string()
                + "17,9,15,3,13,1,12,7,8,2,0,0,0,0,0,0,0,0,0,0,13,1,14,1,15,7,20,3,22,3,0,0,0,0,0,0,0,0,0,0\n").to_string()
        );
    }
}
