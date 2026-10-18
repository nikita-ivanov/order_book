use chrono::{DateTime, Utc};
use std::collections::BTreeMap;

use crate::book::{error::OrderBookError, snapshot::OrderBookSnapshot};

// TODO I made bids and asks public so that we can verify checksum without getting Snapshot
// and avoid copy of vectors as well have access to i64, is that ok?
#[derive(Debug, Clone)]
pub struct OrderBook {
    name: String,
    depth: u16,
    pub bids: BTreeMap<i64, i64>,
    pub asks: BTreeMap<i64, i64>,
    pair_decimals: usize,
    lot_decimals: usize,
    timestamp: DateTime<Utc>,
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
    pub timestamp: DateTime<Utc>,
}

impl OrderBook {
    pub fn new(
        name: String,
        depth: u16,
        pair_decimals: usize,
        lot_decimals: usize,
        timestamp: DateTime<Utc>,
    ) -> OrderBook {
        OrderBook {
            name,
            depth,
            pair_decimals,
            lot_decimals,
            asks: BTreeMap::new(),
            bids: BTreeMap::new(),
            timestamp,
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

        self.timestamp = update.timestamp;

        Ok(())
    }

    pub fn get_snapshot(&self) -> OrderBookSnapshot {
        OrderBookSnapshot::new(
            self.name.clone(),
            self.bids.clone(),
            self.asks.clone(),
            self.timestamp,
            self.pair_decimals,
            self.lot_decimals,
            self.depth,
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::book::snapshot::BidAskPair;

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
            timestamp: Utc::now(),
        }
    }

    fn book_name() -> String {
        String::from("dummy name")
    }

    #[test]
    fn test_order_book_trims_to_depth() {
        let mut book = OrderBook::new(book_name(), 10, 2, 8, Utc::now());

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
        let mut book = OrderBook::new(book_name(), 10, 2, 8, Utc::now());

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
        let mut book = OrderBook::new(book_name(), 10, 2, 8, Utc::now());

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
        let mut book = OrderBook::new("dummy".to_string(), 10, 0, 0, Utc::now());

        // basic one level test
        book.apply_update(get_update(1, 100, 40)).unwrap();

        let top = book.get_snapshot().top_of_book().unwrap();

        // bids: (100, 40)
        // asks: (100, 40)
        let expected = BidAskPair {
            bid_price: 100.0,
            bid_qty: 40.0,
            ask_price: 100.0,
            ask_qty: 40.0,
        };

        assert_eq!(top.bid_price, expected.bid_price);
        assert_eq!(top.bid_qty, expected.bid_qty);
        assert_eq!(top.ask_price, expected.ask_price);
        assert_eq!(top.ask_qty, expected.ask_qty);

        book.apply_update(get_update(1, 101, 41)).unwrap();
        let top = book.get_snapshot().top_of_book().unwrap();

        // bids: (100, 40), (101, 41)
        // asks: (100, 40), (101, 41)
        let expected = BidAskPair {
            bid_price: 101.0,
            bid_qty: 41.0,
            ask_price: 100.0,
            ask_qty: 40.0,
        };

        assert_eq!(top.bid_price, expected.bid_price);
        assert_eq!(top.bid_qty, expected.bid_qty);
        assert_eq!(top.ask_price, expected.ask_price);
        assert_eq!(top.ask_qty, expected.ask_qty);

        book.apply_update(get_update(1, 99, 42)).unwrap();
        let top = book.get_snapshot().top_of_book().unwrap();

        // bids: (100, 40), (101, 41), (99, 42)
        // asks: (100, 40), (101, 41), (99, 42)
        let expected = BidAskPair {
            bid_price: 101.0,
            bid_qty: 41.0,
            ask_price: 99.0,
            ask_qty: 42.0,
        };

        assert_eq!(top.bid_price, expected.bid_price);
        assert_eq!(top.bid_qty, expected.bid_qty);
        assert_eq!(top.ask_price, expected.ask_price);
        assert_eq!(top.ask_qty, expected.ask_qty);

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
            timestamp: Utc::now(),
        })
        .unwrap();

        let top = book.get_snapshot().top_of_book().unwrap();

        // bids: (99, 42)
        // asks: (100, 40), (101, 41)
        let expected = BidAskPair {
            bid_price: 99.0,
            bid_qty: 42.0,
            ask_price: 100.0,
            ask_qty: 40.0,
        };

        assert_eq!(top.bid_price, expected.bid_price);
        assert_eq!(top.bid_qty, expected.bid_qty);
        assert_eq!(top.ask_price, expected.ask_price);
        assert_eq!(top.ask_qty, expected.ask_qty);
    }

    #[test]
    fn test_empty_book_return_error() {
        let mut book = OrderBook::new("dummy".to_string(), 10, 0, 0, Utc::now());

        let Err(OrderBookError::BookEmpty) = book.get_snapshot().top_of_book() else {
            panic!("top of book should error because of empty book");
        };

        book.apply_update(BookUpdate {
            bids: Some(vec![Level {
                price: 24,
                quantity: 42,
            }]),
            asks: None,
            timestamp: Utc::now(),
        })
        .unwrap();

        let Err(OrderBookError::BookEmpty) = book.get_snapshot().top_of_book() else {
            panic!("top of book should error because of empty asks");
        };

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
            timestamp: Utc::now(),
        })
        .unwrap();

        let Err(OrderBookError::BookEmpty) = book.get_snapshot().top_of_book() else {
            panic!("top of book should error because of empty bids");
        };
    }
}
