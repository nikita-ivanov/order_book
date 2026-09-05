use chrono::Utc;
use serde_json::Number;
use std::{collections::BTreeMap, num::ParseIntError};
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum OrderBookError {
    #[error(
        "Received number from exchange with higher precision than expected ({precision}): {number:?}"
    )]
    PrecisionExceeded { precision: usize, number: String },
    #[error("Failed to convert string number to integer: {number}")]
    InvalidNumber {
        number: String,
        #[source]
        source: ParseIntError,
    },
    #[error("Computed checksum ({computed}) does not match received one ({received})")]
    ChecksumMismatch { computed: u32, received: u32 },
    #[error("No bids or asks in the order book")]
    BookEmpty,
}

/// Helper to convert integer order book data to f64
fn int_to_float(number: i64, precision: usize) -> f64 {
    (number as f64) / (10_u32.pow(precision as u32) as f64)
}

// do we like that we need lifetime here because of &String?
#[derive(Debug)]
pub struct OrderBook {
    name: String,
    depth: usize,
    pair_decimals: usize,
    lot_decimals: usize,
    bids: BTreeMap<i64, i64>,
    asks: BTreeMap<i64, i64>,
    verify_checksum: bool,
}

#[derive(Debug)]
pub struct Level {
    price: i64,
    quantity: i64,
}

#[derive(Debug)]
pub struct BookUpdate {
    bids: Option<Vec<Level>>,
    asks: Option<Vec<Level>>,
}

impl OrderBook {
    pub fn new(
        name: String,
        depth: usize,
        pair_decimals: usize,
        lot_decimals: usize,
        verify_checksum: bool,
    ) -> OrderBook {
        OrderBook {
            name,
            depth,
            pair_decimals,
            lot_decimals,
            asks: BTreeMap::new(),
            bids: BTreeMap::new(),
            verify_checksum,
        }
    }

    fn number_to_i64(number: &Number, precision: usize) -> Result<i64, OrderBookError> {
        let number_string = number.to_string();

        let (int_part, frac_part) = match number_string.split_once(".") {
            Some(split) => split,
            None => (number_string.as_str(), "0"),
        };

        let first_part = String::from(int_part);
        let mut second_part = String::from(frac_part);

        // receiving more digits than expected is considered an error
        if second_part.len() > precision {
            return Err(OrderBookError::PrecisionExceeded {
                precision,
                number: number.to_string(),
            });
        }

        while second_part.len() < precision {
            second_part += "0";
        }

        let combined = first_part + &second_part;

        combined
            .parse::<i64>()
            .map_err(|error| OrderBookError::InvalidNumber {
                number: number_string,
                source: error,
            })
    }

    pub fn apply_update(&mut self, update: BookUpdate) -> Result<(), OrderBookError> {
        if let Some(bids) = update.bids {
            for bid in bids {
                match bid.quantity {
                    0 => self.bids.remove(&bid.price),
                    _ => self.bids.insert(bid.price, bid.quantity),
                };
            }
        }

        if let Some(asks) = update.asks {
            for ask in asks {
                match ask.quantity {
                    0 => self.asks.remove(&ask.price),
                    _ => self.asks.insert(ask.price, ask.quantity),
                };
            }
        }

        while self.bids.len() > self.depth {
            self.bids.pop_first();
        }

        while self.asks.len() > self.depth {
            self.asks.pop_last();
        }

        // if self.verify_checksum {
        //     let computed_checksum = self.compute_checksum();
        //     if book_data.checksum != computed_checksum {
        //         return Err(OrderBookError::ChecksumMismatch {
        //             computed: computed_checksum,
        //             received: book_data.checksum,
        //         });
        //     }
        // }

        Ok(())
    }

    pub fn top_of_book(&self) -> Result<BidAskPair, OrderBookError> {
        // TODO: should we instead store reversed bids and use first_key_value()?
        let (best_bid_price, best_bid_qty) = match self.bids.last_key_value() {
            Some(x) => x,
            None => {
                return Err(OrderBookError::BookEmpty);
            }
        };

        let (best_ask_price, best_ask_qty) = match self.asks.first_key_value() {
            Some(x) => x,
            None => {
                return Err(OrderBookError::BookEmpty);
            }
        };

        let best_bid_price = int_to_float(*best_bid_price, self.pair_decimals);
        let best_bid_qty = int_to_float(*best_bid_qty, self.lot_decimals);

        let best_ask_price = int_to_float(*best_ask_price, self.pair_decimals);
        let best_ask_qty = int_to_float(*best_ask_qty, self.lot_decimals);

        let top_of_book = BidAskPair {
            bid_price: best_bid_price,
            bid_qty: best_bid_qty,
            ask_price: best_ask_price,
            ask_qty: best_ask_qty,
        };

        Ok(top_of_book)
    }

    fn compute_checksum(&self) -> u32 {
        // kraken computes checksum on the first 10 entries
        let bids = self.bids.iter().rev().take(10);
        let asks = self.asks.iter().take(10);

        let mut concatenated_asks = String::new();
        for ask in asks {
            let (price, qty) = ask;
            let parsed_price = price.to_string();
            let parsed_qty = qty.to_string();

            concatenated_asks.push_str(&parsed_price);
            concatenated_asks.push_str(&parsed_qty);
        }

        let mut concatenated_bids = String::new();
        for bid in bids {
            let (price, qty) = bid;
            let parsed_price = price.to_string();
            let parsed_qty = qty.to_string();

            concatenated_bids.push_str(&parsed_price);
            concatenated_bids.push_str(&parsed_qty);
        }

        let asks_and_bids = concatenated_asks + &concatenated_bids[..];

        crc32fast::hash(asks_and_bids.as_bytes())
    }

    pub fn print(&self) {
        let mut bids_to_print: Vec<(f64, f64)> = Vec::with_capacity(self.depth);
        let mut asks_to_print: Vec<(f64, f64)> = Vec::with_capacity(self.depth);

        for pair in self.bids.iter().rev() {
            let price_float = int_to_float(*pair.0, self.pair_decimals);
            let qty_float = int_to_float(*pair.1, self.lot_decimals);

            bids_to_print.push((price_float, qty_float));
        }

        for pair in self.asks.iter() {
            let price_float = int_to_float(*pair.0, self.pair_decimals);
            let qty_float = int_to_float(*pair.1, self.lot_decimals);

            asks_to_print.push((price_float, qty_float));
        }

        println!("{}", "=".repeat(100));
        println!("OrderBook {} {:?}", self.name, Utc::now());
        for (bid_pair, ask_pair) in bids_to_print.iter().zip(asks_to_print) {
            println!(
                "q_bid={:<5.2}, p_bid={:.2} | p_ask={:.2}, q_ask={:>5.2}",
                bid_pair.1, bid_pair.0, ask_pair.0, ask_pair.1
            );
        }
        println!("{}", "=".repeat(100));
    }
}

#[derive(Debug, PartialEq)]
pub struct BidAskPair {
    pub bid_price: f64,
    pub bid_qty: f64,
    pub ask_price: f64,
    pub ask_qty: f64,
}

impl BidAskPair {
    pub fn print(&self) {
        println!(
            "q_bid={:<5.2}, p_bid={:.2} | p_ask={:.2}, q_ask={:>5.2}",
            self.bid_qty, self.bid_price, self.ask_price, self.ask_qty
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // fn get_quotes(n: usize, start_price: f64, start_qty: f64) -> Vec<Quote> {
    //     let mut values = Vec::new();
    //
    //     for i in 0..n {
    //         let price = start_price + i as f64;
    //         let qty = start_qty + i as f64;
    //         let quote = Quote {
    //             price: Number::from_str(&price.to_string()[..]).unwrap(),
    //             qty: Number::from_str(&qty.to_string()[..]).unwrap(),
    //         };
    //         values.push(quote);
    //     }
    //
    //     values
    // }

    fn get_update(n: usize, start_price: i64, start_qty: i64) -> BookUpdate {
        let mut bids = Vec::new();
        let mut asks = Vec::new();

        for i in 0..n {
            let price = start_price - i as i64;
            let quantity = start_qty - i as i64;
            let level = Level { price, quantity };
            bids.push(level)
        }

        for i in 0..n {
            let price = start_price + i as i64;
            let quantity = start_qty + i as i64;
            let level = Level { price, quantity };
            asks.push(level)
        }

        BookUpdate {
            bids: Some(bids),
            asks: Some(asks),
        }
    }

    // fn get_book_message(n: usize, start_price: f64, start_qty: f64) -> BookMessage {
    //     let book_data = BookData {
    //         symbol: String::from("X"),
    //         bids: get_quotes(n, start_price, start_qty),
    //         asks: get_quotes(n, start_price, start_qty),
    //         checksum: 42,
    //         timestamp: DateTime::parse_from_rfc3339("2026-06-26T11:00:00+02:00")
    //             .unwrap()
    //             .to_utc(),
    //     };
    //
    //     BookMessage {
    //         channel: String::from("book"),
    //         r#type: String::from("update"),
    //         data: [book_data],
    //     }
    // }

    fn book_name() -> String {
        String::from("dummy name")
    }

    #[test]
    fn test_order_book_trims_to_depth() {
        let mut book = OrderBook::new(book_name(), 10, 2, 8, false);

        let update = get_update(1, 100, 42);
        book.apply_update(update).unwrap();
        assert_eq!(book.bids.len(), 10);
        assert_eq!(book.asks.len(), 10);

        // increase price level to expand the book
        let update = get_update(1, 200, 42);
        book.apply_update(update).unwrap();
        assert_eq!(book.bids.len(), 10);
        assert_eq!(book.asks.len(), 10);
    }

    #[test]
    fn test_order_book_overwrites_quantities() {
        let mut book = OrderBook::new(book_name(), 10, 2, 8, false);

        let update = get_update(2, 100, 42);
        book.apply_update(update).unwrap();

        let update = get_update(2, 100, 42);
        book.apply_update(update).unwrap();

        let expected_qty = 24;

        let bid_level = book.bids.get(&100).unwrap();

        assert_eq!(bid_level, &expected_qty);

        let ask_level = book.asks.get(&100).unwrap();
        assert_eq!(ask_level, &expected_qty);
    }

    #[test]
    fn test_order_book_all_price_levels_present() {
        let mut book = OrderBook::new(book_name(), 10, 2, 8, false);

        let update = get_update(1, 100, 42);
        book.apply_update(update).unwrap();

        let update = get_update(1, 105, 422);
        book.apply_update(update).unwrap();

        assert_eq!(book.bids.get(&100).unwrap(), &42);
        assert_eq!(book.bids.get(&105).unwrap(), &422);
        assert_eq!(book.asks.get(&100).unwrap(), &42);
        assert_eq!(book.asks.get(&105).unwrap(), &422);
    }

    // #[test]
    // fn test_top_of_book_returns_correct_values() {
    //     let mut book = OrderBook::new(book_name(), 10, 2, 8, false);
    //
    //     book.apply_update(&get_book_message(2, 100.0, 42.0))
    //         .unwrap();
    //
    //     // we have two book updates with 100.0 and 101.0 prices
    //     let expected = BidAskPair {
    //         bid_price: 101.0,
    //         bid_qty: 43.0,
    //         ask_price: 100.0,
    //         ask_qty: 42.0,
    //     };
    //     assert_eq!(book.top_of_book().unwrap(), expected);
    //
    //     book.apply_update(&get_book_message(1, 105.0, 2.0)).unwrap();
    //     let expected = BidAskPair {
    //         bid_price: 105.0,
    //         bid_qty: 2.0,
    //         ask_price: 100.0,
    //         ask_qty: 42.0,
    //     };
    //     assert_eq!(book.top_of_book().unwrap(), expected);
    //
    //     book.apply_update(&get_book_message(1, 98.0, 3.0)).unwrap();
    //     let expected = BidAskPair {
    //         bid_price: 105.0,
    //         bid_qty: 2.0,
    //         ask_price: 98.0,
    //         ask_qty: 3.0,
    //     };
    //     assert_eq!(book.top_of_book().unwrap(), expected);
    // }
    //
    // #[test]
    // fn test_empty_top_of_book_returns_error() {
    //     let mut book = OrderBook::new(book_name(), 10, 2, 8, false);
    //     let result = book.top_of_book();
    //     assert_eq!(result, Err(OrderBookError::BookEmpty));
    //
    //     // message below only adds bids but no asks
    //     let book_data = BookData {
    //         symbol: String::from("X"),
    //         bids: get_quotes(1, 100.0, 3.2),
    //         asks: vec![],
    //         checksum: 42,
    //         timestamp: DateTime::parse_from_rfc3339("2026-06-26T11:00:00+02:00")
    //             .unwrap()
    //             .to_utc(),
    //     };
    //
    //     let new_message = BookMessage {
    //         channel: String::from("book"),
    //         r#type: String::from("update"),
    //         data: [book_data],
    //     };
    //
    //     book.apply_update(&new_message).unwrap();
    //     let result = book.top_of_book();
    //     assert_eq!(result, Err(OrderBookError::BookEmpty));
    //
    //     let mut book = OrderBook::new(book_name(), 10, 2, 8, false);
    //
    //     // message below only adds asks but no asks
    //     let book_data = BookData {
    //         symbol: String::from("X"),
    //         bids: vec![],
    //         asks: get_quotes(1, 100.0, 3.2),
    //         checksum: 42,
    //         timestamp: DateTime::parse_from_rfc3339("2026-06-26T11:00:00+02:00")
    //             .unwrap()
    //             .to_utc(),
    //     };
    //
    //     let new_message = BookMessage {
    //         channel: String::from("book"),
    //         r#type: String::from("update"),
    //         data: [book_data],
    //     };
    //
    //     book.apply_update(&new_message).unwrap();
    //     let result = book.top_of_book();
    //
    //     assert_eq!(result, Err(OrderBookError::BookEmpty));
    // }
    //
    // #[test]
    // fn test_number_to_i64_errors_on_too_many_digits() {
    //     // note that below we got 3 precision points while we expect 2
    //     let number = Number::from_f64(100.123).unwrap();
    //     let parsing_result = OrderBook::number_to_i64(&number, 2);
    //     assert_eq!(
    //         parsing_result,
    //         Err(OrderBookError::PrecisionExceeded {
    //             precision: 2,
    //             number: number.to_string()
    //         })
    //     )
    // }
    //
    // /// Test conversion from `Number`s to i64 (which are used to store order book bids and asks)
    // #[test]
    // fn test_number_to_i64_parsing() {
    //     let number = Number::from_f64(0.40423).unwrap();
    //     let parsed = OrderBook::number_to_i64(&number, 8).unwrap();
    //     assert_eq!(parsed, 40423000);
    //
    //     let number = Number::from_f64(22.40423).unwrap();
    //     let parsed = OrderBook::number_to_i64(&number, 8).unwrap();
    //     assert_eq!(parsed, 2240423000);
    //
    //     let number = Number::from_f64(100.0).unwrap();
    //     let parsed = OrderBook::number_to_i64(&number, 2).unwrap();
    //     assert_eq!(parsed, 10000);
    //
    //     let number = Number::from_f64(100.1).unwrap();
    //     let parsed = OrderBook::number_to_i64(&number, 2).unwrap();
    //     assert_eq!(parsed, 10010);
    // }
}

// #[cfg(test)]
// mod tests_checksum {
//     use std::str::FromStr;
//
//     use crate::book::{
//         models::{BookData, Quote},
//         order_book::OrderBookError,
//     };
//     use chrono::DateTime;
//     use serde_json::Number;
//
//     use crate::{BookMessage, book::order_book::OrderBook};
//
//     fn get_book_message() -> BookMessage {
//         // from kraken docs: https://docs.kraken.com/exchange/guides/websockets/book-checksum-v2
//         let msg = serde_json::json!(
//             {
//             "channel": "book",
//             "type": "snapshot",
//             "data": [{
//                 "symbol": "BTC/USD",
//                 "bids": [
//                 {"price": 45283.5, "qty": 0.10000000},
//                 {"price": 45283.4, "qty": 1.54582015},
//                 {"price": 45282.1, "qty": 0.10000000},
//                 {"price": 45281.0, "qty": 0.10000000},
//                 {"price": 45280.3, "qty": 1.54592586},
//                 {"price": 45279.0, "qty": 0.07990000},
//                 {"price": 45277.6, "qty": 0.03310103},
//                 {"price": 45277.5, "qty": 0.30000000},
//                 {"price": 45277.3, "qty": 1.54602737},
//                 {"price": 45276.6, "qty": 0.15445238}
//                 ],
//                 "asks": [
//                 {"price": 45285.2, "qty": 0.00100000},
//                 {"price": 45286.4, "qty": 1.54571953},
//                 {"price": 45286.6, "qty": 1.54571109},
//                 {"price": 45289.6, "qty": 1.54560911},
//                 {"price": 45290.2, "qty": 0.15890660},
//                 {"price": 45291.8, "qty": 1.54553491},
//                 {"price": 45294.7, "qty": 0.04454749},
//                 {"price": 45296.1, "qty": 0.35380000},
//                 {"price": 45297.5, "qty": 0.09945542},
//                 {"price": 45299.5, "qty": 0.18772827}
//                 ],
//                 "checksum": 3310070434_u32,
//                 "timestamp": chrono::prelude::Utc::now()
//             }]
//             }
//         );
//
//         let book_message: BookMessage = serde_json::from_slice(msg.to_string().as_bytes()).unwrap();
//
//         book_message
//     }
//
//     fn book_name() -> String {
//         String::from("dummy name")
//     }
//
//     #[test]
//     fn test_checksum_computed_correctly() {
//         let book_message = get_book_message();
//
//         let mut book = OrderBook::new(book_name(), 10, 1, 8, true);
//         book.apply_update(&book_message).unwrap();
//
//         let computed_checksum = book.compute_checksum();
//         assert_eq!(computed_checksum, book_message.data[0].checksum);
//     }
//
//     #[test]
//     fn test_checksum_ignores_orders_beyond_10th() {
//         let book_message = get_book_message();
//
//         let mut book = OrderBook::new(book_name(), 15, 1, 8, true);
//         book.apply_update(&book_message).unwrap();
//
//         // set bid lower and ask higher than get_book_message() values
//         // to make sure they end up in the tail (11th position) and are ingored
//         let book_data = BookData {
//             symbol: String::from("X"),
//             bids: Vec::from([Quote {
//                 price: Number::from_str("5.0").unwrap(),
//                 qty: Number::from_str("5.0").unwrap(),
//             }]),
//             asks: Vec::from([Quote {
//                 price: Number::from_str("1500000.0").unwrap(),
//                 qty: Number::from_str("5.0").unwrap(),
//             }]),
//             // the checksum on the new message should still be the same since it represents current
//             // oder book
//             checksum: book_message.data[0].checksum,
//             timestamp: DateTime::parse_from_rfc3339("2026-06-26T11:00:00+02:00")
//                 .unwrap()
//                 .to_utc(),
//         };
//
//         let new_message = BookMessage {
//             channel: String::from("book"),
//             r#type: String::from("update"),
//             data: [book_data],
//         };
//
//         book.apply_update(&new_message).unwrap();
//
//         let computed_checksum = book.compute_checksum();
//         assert_eq!(computed_checksum, book_message.data[0].checksum);
//     }
//
//     #[test]
//     fn test_checksum_failed_returns_error() {
//         let mut book = OrderBook::new(book_name(), 10, 2, 8, true);
//         let book_data = get_book_message();
//         let result = book.apply_update(&book_data);
//
//         // get_book_message() returns dummy checksum which will not match correct one
//         assert_eq!(
//             result,
//             Err(OrderBookError::ChecksumMismatch {
//                 computed: book.compute_checksum(),
//                 received: book_data.data[0].checksum
//             })
//         )
//     }
// }
