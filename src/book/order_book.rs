use chrono::Utc;
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
pub fn int_to_float(number: i64, precision: usize) -> f64 {
    (number as f64) / (10_u32.pow(precision as u32) as f64)
}

// TODO could not get rid of the pair and log decimals as they are needed to print the book
// also think they are should be coupled to the book because it is not interpretable without them.
// TODO is it ok that all fields are public?
// TODO would it make sense to add timestamp for the last state as reported by exchange/source?
#[derive(Debug)]
pub struct OrderBook {
    pub name: String,
    pub depth: u16,
    pub bids: BTreeMap<i64, i64>,
    pub asks: BTreeMap<i64, i64>,
    pub pair_decimals: usize,
    pub lot_decimals: usize,
}

#[derive(Debug)]
pub struct Level {
    pub price: i64,
    pub quantity: i64,
}

#[derive(Debug)]
pub struct BookUpdate {
    // TODO could be just empty vectors instead of options
    pub bids: Option<Vec<Level>>,
    pub asks: Option<Vec<Level>>,
}

impl OrderBook {
    pub fn new(name: String, depth: u16, pair_decimals: usize, lot_decimals: usize) -> OrderBook {
        OrderBook {
            name,
            depth,
            pair_decimals,
            lot_decimals,
            asks: BTreeMap::new(),
            bids: BTreeMap::new(),
        }
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

        while self.bids.len() as u16 > self.depth {
            self.bids.pop_first();
        }

        while self.asks.len() as u16 > self.depth {
            self.asks.pop_last();
        }

        Ok(())
    }

    pub fn top_of_book(&self) -> Result<BidAskPair, OrderBookError> {
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

    pub fn print(&self) {
        let mut bids_to_print: Vec<(f64, f64)> = Vec::with_capacity(self.depth as usize);
        let mut asks_to_print: Vec<(f64, f64)> = Vec::with_capacity(self.depth as usize);

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

    fn get_update(n: usize, start_price: i64, start_qty: i64) -> BookUpdate {
        let mut bids = Vec::new();
        let mut asks = Vec::new();

        for i in 0..n {
            let price = start_price + i as i64;
            let quantity = start_qty + i as i64;
            let level = Level { price, quantity };
            bids.push(level)
        }

        for i in 0..n {
            let price = start_price - i as i64;
            let quantity = start_qty + i as i64;
            let level = Level { price, quantity };
            asks.push(level)
        }

        BookUpdate {
            bids: Some(bids),
            asks: Some(asks),
        }
    }

    fn book_name() -> String {
        String::from("dummy name")
    }

    // fn get_update(n: usize, p: i64, q: i64) -> BookUpdate {
    //     let mut bids = Vec::new();
    //     let mut asks = Vec::new();
    //     for i in 1..n {
    //         bids.push((p - (i as i64), (q + i as i64)));
    //         asks.push((p - (i as i64), (q + i as i64)));
    //     }
    //
    //     let bids = bids
    //         .into_iter()
    //         .map(|b| Level {
    //             price: b.0,
    //             quantity: b.1,
    //         })
    //         .collect();
    //
    //     let asks = asks
    //         .into_iter()
    //         .map(|b| Level {
    //             price: b.0,
    //             quantity: b.1,
    //         })
    //         .collect();
    //
    //     BookUpdate {
    //         bids: Some(bids),
    //         asks: Some(asks),
    //     }
    // }

    #[test]
    fn test_order_book_trims_to_depth() {
        let mut book = OrderBook::new(book_name(), 10, 2, 8);

        let update = get_update(10, 100, 42);
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
        let mut book = OrderBook::new(book_name(), 10, 2, 8);

        let update = get_update(2, 100, 42);
        book.apply_update(update).unwrap();

        let update = get_update(2, 100, 42);
        book.apply_update(update).unwrap();

        let expected_qty = 42;

        let bid_level = book.bids.get(&100).unwrap();

        assert_eq!(bid_level, &expected_qty);

        let ask_level = book.asks.get(&100).unwrap();
        assert_eq!(ask_level, &expected_qty);
    }

    #[test]
    fn test_order_book_all_price_levels_present() {
        let mut book = OrderBook::new(book_name(), 10, 2, 8);

        let update = get_update(1, 100, 42);
        book.apply_update(update).unwrap();

        let update = get_update(1, 105, 422);
        book.apply_update(update).unwrap();

        assert_eq!(book.bids.get(&100).unwrap(), &42);
        assert_eq!(book.bids.get(&105).unwrap(), &422);
        assert_eq!(book.asks.get(&100).unwrap(), &42);
        assert_eq!(book.asks.get(&105).unwrap(), &422);
    }

    #[test]
    fn test_top_of_book_returns_correct_values() {
        let mut book = OrderBook::new("dummy".to_string(), 10, 0, 0);

        // basic one level test
        book.apply_update(get_update(1, 100, 40)).unwrap();

        let top = book.top_of_book().unwrap();

        // bids: (100, 40)
        // asks: (100, 40)
        let expected = BidAskPair {
            bid_price: 100.0,
            bid_qty: 40.0,
            ask_price: 100.0,
            ask_qty: 40.0,
        };

        assert_eq!(top, expected);

        book.apply_update(get_update(1, 101, 41)).unwrap();
        let top = book.top_of_book().unwrap();

        // bids: (100, 40), (101, 41)
        // asks: (100, 40), (101, 41)
        let expected = BidAskPair {
            bid_price: 101.0,
            bid_qty: 41.0,
            ask_price: 100.0,
            ask_qty: 40.0,
        };

        assert_eq!(top, expected);

        book.apply_update(get_update(1, 99, 42)).unwrap();
        let top = book.top_of_book().unwrap();

        // bids: (100, 40), (101, 41), (99, 42)
        // asks: (100, 40), (101, 41), (99, 42)
        let expected = BidAskPair {
            bid_price: 101.0,
            bid_qty: 41.0,
            ask_price: 99.0,
            ask_qty: 42.0,
        };

        assert_eq!(top, expected);

        book.apply_update(BookUpdate {
            bids: Some(vec![
                Level {
                    price: 101,
                    quantity: 0,
                },
                Level {
                    price: 100,
                    quantity: 0,
                },
            ]),
            asks: Some(vec![Level {
                price: 99,
                quantity: 0,
            }]),
        })
        .unwrap();

        let top = book.top_of_book().unwrap();

        // bids: (99, 42)
        // asks: (100, 40), (101, 41)
        let expected = BidAskPair {
            bid_price: 99.0,
            bid_qty: 42.0,
            ask_price: 100.0,
            ask_qty: 40.0,
        };

        assert_eq!(top, expected);
    }

    #[test]
    fn test_empty_book_return_error() {
        let mut book = OrderBook::new("dummy".to_string(), 10, 0, 0);

        let top = book.top_of_book();
        assert_eq!(top, Err(OrderBookError::BookEmpty));

        book.apply_update(BookUpdate {
            bids: Some(vec![Level {
                price: 24,
                quantity: 42,
            }]),
            asks: None,
        })
        .unwrap();

        let top = book.top_of_book();
        assert_eq!(top, Err(OrderBookError::BookEmpty));

        // remove prev bid level
        book.apply_update(BookUpdate {
            bids: Some(vec![Level {
                price: 24,
                quantity: 0,
            }]),
            asks: Some(vec![Level {
                price: 24,
                quantity: 42,
            }]),
        })
        .unwrap();

        let top = book.top_of_book();
        assert_eq!(top, Err(OrderBookError::BookEmpty));
    }
}
