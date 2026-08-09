use std::collections::BTreeMap;

use serde_json::Number;
use tokio_tungstenite::tungstenite::http::header::{ValueIter, Values};

use crate::{BookMessage, book::models::BookData};

#[derive(Debug)]
pub struct OrderBook {
    depth: usize,
    pair_decimals: usize,
    lot_decimals: usize,
    bids: BTreeMap<i64, i64>,
    asks: BTreeMap<i64, i64>,
}

impl OrderBook {
    pub fn new(depth: usize) -> OrderBook {
        // TODO: for now we hard code the decimals values, should be injected
        OrderBook {
            depth,
            pair_decimals: 2,
            lot_decimals: 8,
            asks: BTreeMap::new(),
            bids: BTreeMap::new(),
        }
    }

    fn number_to_i64(number: &Number) -> i64 {
        let value_str = number.as_str().replace(".", "");
        // TODO: handle unwrap
        let value_i64 = value_str.parse::<i64>().unwrap();
        value_i64
    }

    fn i64_to_str(value: i64, precision: usize) -> String {
        let denominator = (10 as i64).pow(precision as u32);
        println!("denominator={denominator}");
        let first_half = value / denominator;
        println!("first_half={first_half}");
        let second_half = value % denominator;
        println!("seconds_half={second_half}");
        let parsed = first_half.to_string() + "." + &second_half.to_string()[..];
        parsed
    }

    pub fn apply_update(&mut self, update_data: &BookMessage) {
        // TODO: not sure I like data[0] access, though it is currently quaranteed to have exactly 1 element
        let bids = &update_data.data[0].bids;
        let asks = &update_data.data[0].asks;

        for bid in bids {
            let price = OrderBook::number_to_i64(&bid.price);
            let qty = OrderBook::number_to_i64(&bid.qty);
            match qty {
                0 => self.bids.remove(&price),
                _ => self.bids.insert(price, qty),
            };
        }

        for ask in asks {
            let price = OrderBook::number_to_i64(&ask.price);
            let qty = OrderBook::number_to_i64(&ask.qty);
            match qty {
                0 => self.asks.remove(&price),
                _ => self.asks.insert(price, qty),
            };
        }

        while self.bids.len() > self.depth {
            println!("Trimming bids from {} to {}", self.bids.len(), self.depth);
            self.bids.pop_first();
        }

        while self.asks.len() > self.depth {
            println!("Trimming asks from {} to {}", self.asks.len(), self.depth);
            self.asks.pop_last();
        }
    }

    pub fn top_of_book(&self) -> BidAskPair {
        // TODO: should we instead store reversed bids and use first_key_value()?
        let (best_bid_price, best_bid_qty) = match self.bids.last_key_value() {
            Some(x) => x,
            None => {
                panic!("No bids in the order book, failed to get top of book");
            }
        };

        let (best_ask_price, best_ask_qty) = match self.asks.first_key_value() {
            Some(x) => x,
            None => {
                panic!("No asks in the order book, failed to get top of book");
            }
        };

        BidAskPair {
            bid_price: *best_bid_price,
            bid_qty: *best_bid_qty,
            ask_price: *best_ask_price,
            ask_qty: *best_ask_qty,
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct BidAskPair {
    pub bid_price: i64,
    pub bid_qty: i64,
    pub ask_price: i64,
    pub ask_qty: i64,
}

// TODO: we currently compute checksum on the message while we should compute or OrderBook
fn compute_book_checksum(data: &BookData) -> u32 {
    let bids = &data.bids;
    let asks = &data.asks;

    let bids_strings: Vec<String> = bids
        .iter()
        .rev()
        .map(|q| q.price.to_string().replace(".", "") + &q.qty.to_string().replace(".", "")[..])
        .collect();

    let asks_strings: Vec<String> = asks
        .iter()
        .map(|q| q.price.to_string().replace(".", "") + &q.qty.to_string().replace(".", "")[..])
        .collect();

    let concatenated = bids_strings.concat() + &asks_strings.concat()[..];
    let hashed = crc32fast::hash(concatenated[..].as_bytes());

    hashed
}

#[cfg(test)]
mod tests {
    use chrono::DateTime;
    use serde_json::Number;

    use super::*;
    use crate::book::models::{BookData, Quote};

    fn get_quotes(n: usize, start_price: f64, start_qty: f64) -> Vec<Quote> {
        let mut values = Vec::new();

        for i in 0..n {
            let price = start_price + i as f64;
            let qty = start_qty + i as f64;
            let quote = Quote {
                price: Number::from_f64(price).unwrap(),
                qty: Number::from_f64(qty).unwrap(),
            };
            values.push(quote);
        }

        values
    }

    fn get_book_message(n: usize, start_price: f64, start_qty: f64) -> BookMessage {
        let book_data = BookData {
            symbol: String::from("X"),
            bids: get_quotes(n, start_price, start_qty),
            asks: get_quotes(n, start_price, start_qty),
            checksum: 42,
            timestamp: DateTime::parse_from_rfc3339("2026-06-26T11:00:00+02:00")
                .unwrap()
                .to_utc(),
        };

        let book_message = BookMessage {
            channel: String::from("book"),
            r#type: String::from("update"),
            data: [book_data],
        };

        book_message
    }

    #[test]
    fn test_order_book_trims_to_depth() {
        let mut book = OrderBook::new(10);

        book.apply_update(&get_book_message(10, 100.0, 42.0));
        assert_eq!(book.bids.len(), 10);
        assert_eq!(book.asks.len(), 10);

        // increase price level to expand the book
        book.apply_update(&get_book_message(1, 200.0, 42.0));
        assert_eq!(book.bids.len(), 10);
        assert_eq!(book.asks.len(), 10);

        book.apply_update(&get_book_message(2, 300.0, 42.0));
        assert_eq!(book.bids.len(), 10);
        assert_eq!(book.asks.len(), 10);
    }

    #[test]
    fn test_order_book_overwrites_quantities() {
        let mut book = OrderBook::new(10);

        book.apply_update(&get_book_message(2, 100.0, 42.0));
        book.apply_update(&get_book_message(2, 100.0, 24.0));

        let bid_level = book.bids.get(&10000).unwrap();
        assert_eq!(bid_level, &24);

        let ask_level = book.asks.get(&10000).unwrap();
        assert_eq!(ask_level, &24);
    }

    #[test]
    fn test_order_book_all_price_levels_present() {
        let mut book = OrderBook::new(10);

        book.apply_update(&get_book_message(1, 100.0, 42.0));
        book.apply_update(&get_book_message(1, 105.0, 422.0));

        assert_eq!(book.bids.get(&10000).unwrap(), &42);
        assert_eq!(book.bids.get(&10500).unwrap(), &422);
        assert_eq!(book.asks.get(&10000).unwrap(), &42);
        assert_eq!(book.asks.get(&10500).unwrap(), &422);
    }

    #[test]
    fn test_top_of_book_returns_correct_values() {
        let mut book = OrderBook::new(10);

        book.apply_update(&get_book_message(2, 100.0, 42.0));

        // we have two book updates with 100.0 and 101.0 prices
        let expected = BidAskPair {
            bid_price: 10100,
            bid_qty: 43,
            ask_price: 10000,
            ask_qty: 42,
        };
        assert_eq!(book.top_of_book(), expected);

        book.apply_update(&get_book_message(1, 105.0, 2.0));
        let expected = BidAskPair {
            bid_price: 10500,
            bid_qty: 2,
            ask_price: 10000,
            ask_qty: 42,
        };
        assert_eq!(book.top_of_book(), expected);

        book.apply_update(&get_book_message(1, 98.0, 3.0));
        let expected = BidAskPair {
            bid_price: 10500,
            bid_qty: 2,
            ask_price: 9800,
            ask_qty: 3,
        };
        assert_eq!(book.top_of_book(), expected);
    }
}

#[cfg(test)]
mod tests_checksum {
    use crate::{
        BookMessage,
        book::{
            models::BookData,
            order_book::{OrderBook, compute_book_checksum},
        },
    };

    fn get_book_data() -> BookData {
        // from kraken docs: https://docs.kraken.com/exchange/guides/websockets/book-checksum-v2
        let msg = serde_json::json!(
            {
              "channel": "book",
              "type": "snapshot",
              "data": [{
                "symbol": "BTC/USD",
                "bids": [
                  {"price": "45283.5", "qty": "0.10000000"},
                  {"price": "45283.4", "qty": "1.54582015"},
                  {"price": "45282.1", "qty": "0.10000000"},
                  {"price": "45281.0", "qty": "0.10000000"},
                  {"price": "45280.3", "qty": "1.54592586"},
                  {"price": "45279.0", "qty": "0.07990000"},
                  {"price": "45277.6", "qty": "0.03310103"},
                  {"price": "45277.5", "qty": "0.30000000"},
                  {"price": "45277.3", "qty": "1.54602737"},
                  {"price": "45276.6", "qty": "0.15445238"}
                ],
                "asks": [
                  {"price": "45285.2", "qty": "0.00100000"},
                  {"price": "45286.4", "qty": "1.54571953"},
                  {"price": "45286.6", "qty": "1.54571109"},
                  {"price": "45289.6", "qty": "1.54560911"},
                  {"price": "45290.2", "qty": "0.15890660"},
                  {"price": "45291.8", "qty": "1.54553491"},
                  {"price": "45294.7", "qty": "0.04454749"},
                  {"price": "45296.1", "qty": "0.35380000"},
                  {"price": "45297.5", "qty": "0.09945542"},
                  {"price": "45299.5", "qty": "0.18772827"}
                ],
                "checksum": 3310070434 as u32
              }]
            }
        );

        let book_message: BookMessage = serde_json::from_slice(msg.to_string().as_bytes()).unwrap();

        // is it ok to do that trick to get ownership of data?
        book_message.data.into_iter().nth(0).unwrap()
    }
    // TODO: finish test

    // is it a smell to test private methods?
    #[test]
    fn test_i64_to_string_parsing() {
        let number: i64 = 40423;
        let parsed = OrderBook::i64_to_str(number, 8);
        assert_eq!(parsed, "0.40423000");
    }
}
