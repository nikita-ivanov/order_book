use crate::BookMessage;
use crc32fast;
use serde_json::Number;
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct OrderBook {
    depth: usize,
    pair_decimals: usize,
    lot_decimals: usize,
    bids: BTreeMap<i64, i64>,
    asks: BTreeMap<i64, i64>,
}

impl OrderBook {
    pub fn new(depth: usize, pair_decimals: usize, lot_decimals: usize) -> OrderBook {
        OrderBook {
            depth,
            pair_decimals,
            lot_decimals,
            asks: BTreeMap::new(),
            bids: BTreeMap::new(),
        }
    }

    fn number_to_i64(number: &Number, precision: usize) -> i64 {
        let number_string = number.to_string();
        let (first_part, second_part) = match number_string.contains(".") {
            true => number_string.split_once(".").unwrap(),
            false => (number_string.as_str(), "0"),
        };
        let first_part = String::from(first_part);
        let mut second_part = String::from(second_part);

        while second_part.len() < precision {
            second_part += "0";
        }

        let combined = first_part + &second_part[..];

        // TODO: handle unwrap gracefully
        combined.parse::<i64>().unwrap()
    }

    pub fn apply_update(&mut self, update_data: &BookMessage) {
        // TODO: not sure I like data[0] access, though it is currently quaranteed to have exactly 1 element
        let book_data = &update_data.data[0];

        for bid in &book_data.bids {
            let price = OrderBook::number_to_i64(&bid.price, self.pair_decimals);
            let qty = OrderBook::number_to_i64(&bid.qty, self.lot_decimals);
            match qty {
                0 => self.bids.remove(&price),
                _ => self.bids.insert(price, qty),
            };
        }

        for ask in &book_data.asks {
            let price = OrderBook::number_to_i64(&ask.price, self.pair_decimals);
            let qty = OrderBook::number_to_i64(&ask.qty, self.lot_decimals);
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

        println!(
            "current depth: bids {}, asks {}",
            self.bids.len(),
            self.asks.len()
        );

        // TODO: check checksum and return Error if does not match
        let computed_checksum = self.compute_checksum();
        if book_data.checksum != computed_checksum {
            panic!(
                "checksum does not match: computed {computed_checksum} vs. received {}",
                book_data.checksum
            );
        } else {
            println!(
                "checksums match! computed {} vs. received {}",
                computed_checksum, book_data.checksum
            );
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

    pub fn compute_checksum(&self) -> u32 {
        // kraken computes checksum on the first 10 entries
        let bids = self.bids.iter().rev().take(10);
        let asks = self.asks.iter().take(10);

        let mut concatenated_asks = String::new();
        for ask in asks {
            let (price, qty) = ask;
            let parsed_price = price.to_string();
            let parsed_qty = qty.to_string();

            concatenated_asks += &parsed_price[..];
            concatenated_asks += &parsed_qty[..];
        }

        let mut concatenated_bids = String::new();
        for bid in bids {
            let (price, qty) = bid;
            let parsed_price = price.to_string();
            let parsed_qty = qty.to_string();

            concatenated_bids += &parsed_price[..];
            concatenated_bids += &parsed_qty[..];
        }

        let asks_and_bids = concatenated_asks + &concatenated_bids[..];

        crc32fast::hash(asks_and_bids.as_bytes())
    }
}

#[derive(Debug, PartialEq)]
pub struct BidAskPair {
    pub bid_price: i64,
    pub bid_qty: i64,
    pub ask_price: i64,
    pub ask_qty: i64,
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

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
                price: Number::from_str(&price.to_string()[..]).unwrap(),
                qty: Number::from_str(&qty.to_string()[..]).unwrap(),
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

        BookMessage {
            channel: String::from("book"),
            r#type: String::from("update"),
            data: [book_data],
        }
    }

    fn str_to_i64(s: &str, precision: usize) -> i64 {
        let number = Number::from_str(s).unwrap();
        // TODO: is it ok we use private method here?
        OrderBook::number_to_i64(&number, precision)
    }

    #[test]
    fn test_order_book_trims_to_depth() {
        let mut book = OrderBook::new(10, 2, 8);

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
        let mut book = OrderBook::new(10, 2, 8);

        book.apply_update(&get_book_message(2, 100.0, 42.0));
        book.apply_update(&get_book_message(2, 100.0, 24.0));

        let expected_qty = str_to_i64("24", book.lot_decimals);

        let bid_level = book
            .bids
            .get(&str_to_i64("100.0", book.pair_decimals))
            .unwrap();

        assert_eq!(bid_level, &expected_qty);

        let ask_level = book
            .asks
            .get(&str_to_i64("100.0", book.pair_decimals))
            .unwrap();
        assert_eq!(ask_level, &expected_qty);
    }

    #[test]
    fn test_order_book_all_price_levels_present() {
        let mut book = OrderBook::new(10, 2, 8);

        book.apply_update(&get_book_message(1, 100.0, 42.0));
        book.apply_update(&get_book_message(1, 105.0, 422.0));

        assert_eq!(
            book.bids
                .get(&str_to_i64("100.0", book.pair_decimals))
                .unwrap(),
            &str_to_i64("42.0", book.lot_decimals)
        );

        assert_eq!(
            book.bids
                .get(&str_to_i64("105.0", book.pair_decimals))
                .unwrap(),
            &str_to_i64("422.0", book.lot_decimals)
        );

        assert_eq!(
            book.bids
                .get(&str_to_i64("100.0", book.pair_decimals))
                .unwrap(),
            &str_to_i64("42.0", book.lot_decimals)
        );

        assert_eq!(
            book.bids
                .get(&str_to_i64("105.0", book.pair_decimals))
                .unwrap(),
            &str_to_i64("422.0", book.lot_decimals)
        );
    }

    #[test]
    fn test_top_of_book_returns_correct_values() {
        let mut book = OrderBook::new(10, 2, 8);

        book.apply_update(&get_book_message(2, 100.0, 42.0));

        // we have two book updates with 100.0 and 101.0 prices
        let expected = BidAskPair {
            bid_price: 10100,
            bid_qty: 4300000000,
            ask_price: str_to_i64("100.0", book.pair_decimals),
            ask_qty: str_to_i64("42", book.lot_decimals),
        };
        assert_eq!(book.top_of_book(), expected);

        book.apply_update(&get_book_message(1, 105.0, 2.0));
        let expected = BidAskPair {
            bid_price: str_to_i64("105.0", book.pair_decimals),
            bid_qty: str_to_i64("2", book.lot_decimals),
            ask_price: str_to_i64("100.0", book.pair_decimals),
            ask_qty: str_to_i64("42", book.lot_decimals),
        };
        assert_eq!(book.top_of_book(), expected);

        book.apply_update(&get_book_message(1, 98.0, 3.0));
        let expected = BidAskPair {
            bid_price: str_to_i64("105.0", book.pair_decimals),
            bid_qty: str_to_i64("2", book.lot_decimals),
            ask_price: str_to_i64("98.0", book.pair_decimals),
            ask_qty: str_to_i64("3", book.lot_decimals),
        };
        assert_eq!(book.top_of_book(), expected);
    }
}

#[cfg(test)]
mod tests_checksum {
    use std::str::FromStr;

    use crate::book::{
        models::{BookData, Quote},
        order_book::tests,
    };
    use chrono::DateTime;
    use serde_json::Number;

    use crate::{BookMessage, book::order_book::OrderBook};

    fn get_book_message() -> BookMessage {
        // from kraken docs: https://docs.kraken.com/exchange/guides/websockets/book-checksum-v2
        let msg = serde_json::json!(
            {
            "channel": "book",
            "type": "snapshot",
            "data": [{
                "symbol": "BTC/USD",
                "bids": [
                {"price": 45283.5, "qty": 0.10000000},
                {"price": 45283.4, "qty": 1.54582015},
                {"price": 45282.1, "qty": 0.10000000},
                {"price": 45281.0, "qty": 0.10000000},
                {"price": 45280.3, "qty": 1.54592586},
                {"price": 45279.0, "qty": 0.07990000},
                {"price": 45277.6, "qty": 0.03310103},
                {"price": 45277.5, "qty": 0.30000000},
                {"price": 45277.3, "qty": 1.54602737},
                {"price": 45276.6, "qty": 0.15445238}
                ],
                "asks": [
                {"price": 45285.2, "qty": 0.00100000},
                {"price": 45286.4, "qty": 1.54571953},
                {"price": 45286.6, "qty": 1.54571109},
                {"price": 45289.6, "qty": 1.54560911},
                {"price": 45290.2, "qty": 0.15890660},
                {"price": 45291.8, "qty": 1.54553491},
                {"price": 45294.7, "qty": 0.04454749},
                {"price": 45296.1, "qty": 0.35380000},
                {"price": 45297.5, "qty": 0.09945542},
                {"price": 45299.5, "qty": 0.18772827}
                ],
                "checksum": 3310070434_u32,
                "timestamp": chrono::prelude::Utc::now()
            }]
            }
        );

        let book_message: BookMessage = serde_json::from_slice(msg.to_string().as_bytes()).unwrap();

        book_message
    }

    #[test]
    fn test_checksum_computed_correctly() {
        let book_message = get_book_message();

        let mut book = OrderBook::new(10, 1, 8);
        book.apply_update(&book_message);

        let computed_checksum = book.compute_checksum();
        assert_eq!(computed_checksum, book_message.data[0].checksum);
    }

    #[test]
    fn test_checksum_ignores_orders_beyond_10th() {
        let book_message = get_book_message();

        let mut book = OrderBook::new(10, 1, 8);
        book.apply_update(&book_message);

        // set bid lower and ask higher than get_book_message() values
        // to make sure they end up in the tail (11th position) and are ingored
        let book_data = BookData {
            symbol: String::from("X"),
            bids: Vec::from([Quote {
                price: Number::from_str("5.0").unwrap(),
                qty: Number::from_str("5.0").unwrap(),
            }]),
            asks: Vec::from([Quote {
                price: Number::from_str("1500000.0").unwrap(),
                qty: Number::from_str("5.0").unwrap(),
            }]),
            checksum: 42,
            timestamp: DateTime::parse_from_rfc3339("2026-06-26T11:00:00+02:00")
                .unwrap()
                .to_utc(),
        };

        let new_message = BookMessage {
            channel: String::from("book"),
            r#type: String::from("update"),
            data: [book_data],
        };

        book.apply_update(&new_message);

        let computed_checksum = book.compute_checksum();
        assert_eq!(computed_checksum, book_message.data[0].checksum);
    }

    /// Test conversion from `Number`s to i64 (which are used to store order book bids and asks)
    #[test]
    fn test_number_to_i64_parsing() {
        let number = Number::from_f64(0.40423).unwrap();
        let parsed = OrderBook::number_to_i64(&number, 8);
        assert_eq!(parsed, 40423000);

        let number = Number::from_f64(22.40423).unwrap();
        let parsed = OrderBook::number_to_i64(&number, 8);
        assert_eq!(parsed, 2240423000);

        let number = Number::from_f64(100.0).unwrap();
        let parsed = OrderBook::number_to_i64(&number, 2);
        assert_eq!(parsed, 10000);

        let number = Number::from_f64(100.1).unwrap();
        let parsed = OrderBook::number_to_i64(&number, 2);
        assert_eq!(parsed, 10010);
    }

    #[test]
    #[should_panic]
    fn test_number_to_i64_parsing_panics_on_too_many_digits() {
        // note that below we got 3 precision points while we expect 2
        let number = Number::from_f64(100.123).unwrap();
        let parsed = OrderBook::number_to_i64(&number, 2);
        assert_eq!(parsed, 10012);
    }
}
