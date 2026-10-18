use std::collections::BTreeMap;

use chrono::{DateTime, Utc};

use crate::book::{core::int_to_float, error::OrderBookError};

pub struct OrderBookSnapshot {
    pub depth: u16,
    pub timestamp: DateTime<Utc>,
    name: String,
    bids: BTreeMap<i64, i64>,
    asks: BTreeMap<i64, i64>,
    pair_decimals: usize,
    lot_decimals: usize,
}

#[derive(Debug)]
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

impl OrderBookSnapshot {
    pub fn new(
        name: String,
        bids: BTreeMap<i64, i64>,
        asks: BTreeMap<i64, i64>,
        timestamp: DateTime<Utc>,
        pair_decimals: usize,
        lot_decimals: usize,
        depth: u16,
    ) -> OrderBookSnapshot {
        OrderBookSnapshot {
            name,
            bids,
            asks,
            timestamp,
            pair_decimals,
            lot_decimals,
            depth,
        }
    }

    pub fn get_bids(&self) -> Vec<(f64, f64)> {
        let mut bids: Vec<(f64, f64)> = Vec::with_capacity(self.depth as usize);

        for pair in self.bids.iter().rev() {
            let price_float = int_to_float(*pair.0, self.pair_decimals);
            let qty_float = int_to_float(*pair.1, self.lot_decimals);

            bids.push((price_float, qty_float));
        }

        bids
    }

    pub fn get_asks(&self) -> Vec<(f64, f64)> {
        let mut asks: Vec<(f64, f64)> = Vec::with_capacity(self.depth as usize);

        for pair in self.asks.iter() {
            let price_float = int_to_float(*pair.0, self.pair_decimals);
            let qty_float = int_to_float(*pair.1, self.lot_decimals);

            asks.push((price_float, qty_float));
        }

        asks
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
        let bids = self.get_bids();
        let asks = self.get_asks();

        println!("{}", "=".repeat(100));
        println!("OrderBook {} {:?}", self.name, self.timestamp);

        for (bid_pair, ask_pair) in bids.iter().zip(asks) {
            println!(
                "q_bid={:<5.2}, p_bid={:.2} | p_ask={:.2}, q_ask={:>5.2}",
                bid_pair.1, bid_pair.0, ask_pair.0, ask_pair.1
            );
        }

        println!("{}", "=".repeat(100));
    }
}
