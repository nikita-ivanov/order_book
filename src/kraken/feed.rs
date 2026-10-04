use anyhow::{Result, anyhow};
use futures_util::StreamExt;
use serde_json::Number;
use std::{collections::HashMap, num::ParseIntError};
use thiserror::Error;
use tokio_tungstenite::tungstenite::Message;

use crate::kraken::core::{Receiver, connect, request_channels};
use crate::{
    book::order_book::{BookUpdate, Level, OrderBook, OrderBookError},
    kraken::{
        models::{BasicInfo, BasicInfoResult, BookMessage},
        precision::{AssetPairPrecision, get_precision},
    },
    store::OrderBookStore,
};

#[derive(Debug, Error, PartialEq)]
pub enum KrakenFeedError {
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

    #[error("Error applying update to the order book: {source:?}")]
    OrderBookError {
        #[source]
        source: OrderBookError,
    },

    // TODO can we pass the actual error message here?
    // tried to use serde_json::Error as source but it does not implement PartialEq
    #[error("Error deserializing exchange message. Message was {message:?}")]
    DeserializationError { message: Message },

    #[error("Unknown message: {message}")]
    UnknownMessage { message: Message },
}

pub struct KrakenFeed {
    pub symbols: Vec<String>,
    pub depth: u16,
    pub order_books: HashMap<String, OrderBook>,
    precisions: HashMap<String, AssetPairPrecision>,
    verify_checksum: bool,
    stores: Option<HashMap<String, OrderBookStore>>,
}

impl KrakenFeed {
    // TODO It would probably make more sense to restrict one feed to one symbol?
    pub async fn new(
        symbols: &[String],
        depth: u16,
        verify_checksum: bool,
        stores: HashMap<String, OrderBookStore>,
    ) -> Result<KrakenFeed> {
        let precisions = get_precision(symbols).await?;
        let mut books = HashMap::<String, OrderBook>::new();

        for (symbol, precision) in precisions.iter() {
            // TODO I don't like that we have ~5 string allocations here

            let ob = OrderBook::new(
                String::from(symbol),
                depth,
                precision.pair_decimals,
                precision.lot_decimals,
            );

            books.insert(String::from(symbol), ob);
        }

        Ok(KrakenFeed {
            symbols: Vec::from(symbols),
            depth,
            precisions,
            order_books: books,
            verify_checksum,
            stores: Some(stores),
        })
    }

    fn process(&mut self, message: Message) -> Result<&OrderBook, KrakenFeedError> {
        let Message::Text(bytes) = &message else {
            println!("Message was not of the text type, will skip. {message:?}");
            return Err(KrakenFeedError::UnknownMessage { message });
        };

        let info = match serde_json::from_str::<BasicInfo>(bytes) {
            Ok(parsed) => parsed,
            Err(_) => {
                return Err(KrakenFeedError::DeserializationError { message });
            }
        };

        // could be a match here to print the result of parse()?
        let (BasicInfoResult::Snapshot | BasicInfoResult::Update) = info.parse() else {
            return Err(KrakenFeedError::UnknownMessage { message });
        };

        let message = match serde_json::from_str::<BookMessage>(bytes.as_str()) {
            Ok(m) => m,
            Err(_) => {
                return Err(KrakenFeedError::DeserializationError { message });
            }
        };

        let symbol = &message.data[0].symbol;
        let order_book = match self.order_books.get_mut(symbol) {
            Some(book) => book,
            None => panic!("could not find order book {symbol}"),
        };

        let precisions = self.precisions.get(symbol).unwrap();

        let mut bids = Vec::<Level>::new();
        for bid in &message.data[0].bids {
            let price = KrakenFeed::number_to_i64(&bid.price, precisions.pair_decimals)?;
            let quantity = KrakenFeed::number_to_i64(&bid.qty, precisions.lot_decimals)?;
            bids.push(Level { price, quantity })
        }

        let mut asks = Vec::<Level>::new();
        for ask in &message.data[0].asks {
            let price = KrakenFeed::number_to_i64(&ask.price, precisions.pair_decimals)?;
            let quantity = KrakenFeed::number_to_i64(&ask.qty, precisions.lot_decimals)?;
            asks.push(Level { price, quantity })
        }

        let book_update = BookUpdate {
            bids: Some(bids),
            asks: Some(asks),
        };

        let _ = match order_book.apply_update(book_update) {
            Ok(()) => Ok(()),
            Err(e) => Err(KrakenFeedError::OrderBookError { source: e }),
        };

        if self.verify_checksum {
            let bids: &Vec<(i64, i64)> = &order_book
                .bids
                .iter()
                .map(|pair| (*pair.0, *pair.1))
                .collect();

            let asks: &Vec<(i64, i64)> = &order_book
                .asks
                .iter()
                .map(|pair| (*pair.0, *pair.1))
                .collect();

            let computed_checksum = KrakenFeed::compute_checksum(bids, asks);

            let received_checksum = message.data[0].checksum;

            if computed_checksum != received_checksum {
                return Err(KrakenFeedError::ChecksumMismatch {
                    computed: computed_checksum,
                    received: received_checksum,
                });
            }
        };

        if let Some(stores) = &mut self.stores {
            let store = stores
                .get_mut(symbol)
                .expect("Store {symbol} should be there");

            store.save(order_book);
        }

        Ok(order_book)
    }

    async fn receiving_loop(&mut self, rx: &mut Receiver) -> Result<()> {
        // TODO handle update/snapshot messages differently?
        while let Some(msg) = rx.next().await {
            let msg = match msg {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("Websocket error, will skip. Error was: {e:?}");
                    continue;
                }
            };

            match self.process(msg) {
                Ok(ob) => {
                    ob.print();

                    continue;
                }

                Err(KrakenFeedError::UnknownMessage { message }) => {
                    println!("Unknown message, will skip.\n{message:?}");
                    continue;
                }

                Err(KrakenFeedError::BookEmpty) => {
                    // this is unreachable
                    continue;
                }

                Err(KrakenFeedError::InvalidNumber { number, source }) => {
                    println!(
                        "Received invalid number during parsing: {number:?}. Parsing error was {source:?}. Will skip."
                    );
                    continue;
                }

                Err(KrakenFeedError::DeserializationError { message }) => {
                    println!("Failed to deserialize message. Will skip. Message was {message:?}");
                    continue;
                }

                Err(KrakenFeedError::ChecksumMismatch { computed, received }) => {
                    anyhow!(
                        "Checksum mismatch: received {received:?} vs. computed {computed:?}. Try to reconnect."
                    )
                }

                Err(KrakenFeedError::PrecisionExceeded { precision, number }) => {
                    anyhow!(
                        "Exchange sent a number with higher precision than expected, which is considered a breaking error.
                        Precision was {precision} while received number was {number}"
                    )
                }

                Err(KrakenFeedError::OrderBookError { source }) => {
                    anyhow!(
                        "Error during order book processing, try to reconnect. Error was\n{source:?}"
                    )
                }
            };
        }

        Err(anyhow!("websocket exited, try to reconnect"))
    }

    pub async fn run_once(&mut self) -> Result<()> {
        let ws = connect().await?;
        let (mut tx, mut rx) = ws.split();

        request_channels(&mut tx, &self.symbols, self.depth as usize).await?;

        match self.receiving_loop(&mut rx).await {
            Ok(()) => Ok(()),
            Err(other) => Err(anyhow!("received unexpected error during loop: {other:?}")),
        }
    }

    fn compute_checksum(bids: &[(i64, i64)], asks: &[(i64, i64)]) -> u32 {
        // kraken computes checksum on the first 10 entries
        let bids = bids.iter().rev().take(10);
        let asks = asks.iter().take(10);

        let mut concatenated_asks = String::new();
        for ask in asks {
            let parsed_price = ask.0.to_string();
            let parsed_qty = ask.1.to_string();

            concatenated_asks.push_str(&parsed_price);
            concatenated_asks.push_str(&parsed_qty);
        }

        let mut concatenated_bids = String::new();
        for bid in bids {
            let parsed_price = bid.0.to_string();
            let parsed_qty = bid.1.to_string();

            concatenated_bids.push_str(&parsed_price);
            concatenated_bids.push_str(&parsed_qty);
        }

        let asks_and_bids = concatenated_asks + &concatenated_bids[..];

        crc32fast::hash(asks_and_bids.as_bytes())
    }

    fn number_to_i64(number: &Number, precision: usize) -> Result<i64, KrakenFeedError> {
        let number_string = number.to_string();

        let (int_part, frac_part) = match number_string.split_once(".") {
            Some(split) => split,
            None => (number_string.as_str(), "0"),
        };

        let first_part = String::from(int_part);
        let mut second_part = String::from(frac_part);

        // receiving more digits than expected is considered an error
        if second_part.len() > precision {
            return Err(KrakenFeedError::PrecisionExceeded {
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
            .map_err(|error| KrakenFeedError::InvalidNumber {
                number: number_string,
                source: error,
            })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};
    use tokio_tungstenite::tungstenite::{Bytes, Message, Utf8Bytes};

    use crate::{
        book::order_book::OrderBook,
        kraken::{
            feed::{KrakenFeed, KrakenFeedError},
            precision::AssetPairPrecision,
        },
    };

    fn get_book_message() -> Message {
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

        let message_bytes =
            unsafe { Utf8Bytes::from_bytes_unchecked(Bytes::from(msg.to_string())) };

        Message::Text(message_bytes)
    }

    fn get_dummy_book_message(bid: f64, ask: f64) -> Message {
        let msg = serde_json::json!(
            {
            "channel": "book",
            "type": "snapshot",
            "data": [{
                "symbol": "BTC/USD",
                "bids": [
                {"price": bid, "qty": 42.0},
                ],
                "asks": [
                {"price": ask, "qty": 24.0},
                ],
                // this has the same checksum as original
                "checksum": 3310070434_u32,
                "timestamp": chrono::prelude::Utc::now()
            }]
            }
        );

        let message_bytes =
            unsafe { Utf8Bytes::from_bytes_unchecked(Bytes::from(msg.to_string())) };

        Message::Text(message_bytes)
    }

    fn get_feed(depth: u16) -> KrakenFeed {
        let mut order_books = HashMap::new();

        let ob = OrderBook {
            name: "dummy order book".to_string(),
            depth,
            pair_decimals: 1,
            lot_decimals: 2,
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
        };
        order_books.insert("BTC/USD".to_string(), ob);

        let mut precisions = HashMap::new();
        precisions.insert(
            "BTC/USD".to_string(),
            AssetPairPrecision {
                pair_decimals: 1,
                lot_decimals: 8,
            },
        );

        KrakenFeed {
            depth,
            order_books,
            precisions,
            verify_checksum: true,
            symbols: Vec::from(["BTC/USD".to_string()]),
            stores: None,
        }
    }

    #[test]
    fn test_full_message_prcessing_passes_checksum() {
        let book_message = get_book_message();

        let mut feed = get_feed(10);

        // this will verify checksum
        feed.process(book_message).unwrap();
    }

    #[test]
    fn test_checksum_ignores_orders_beyond_10th() {
        let mut feed = get_feed(20);

        let real_message = get_book_message();
        feed.process(real_message).unwrap();

        // this should push (low bid and high ask) levels beyond 10th and ignore them for checksum
        let dummy_message = get_dummy_book_message(100.0, 100_000.0);
        feed.process(dummy_message).unwrap();
    }

    #[test]
    fn test_checksum_mismatch_returns_error() {
        let mut feed = get_feed(20);

        let real_message = get_book_message();
        feed.process(real_message).unwrap();

        let mut feed = get_feed(20);

        let real_message = get_book_message();
        let _ = feed.process(real_message).unwrap();

        // the dummy message below should lend inside 10 levels and hence go into checksum
        // calculation; this should result in mismatched checksums
        let dummy_message = get_dummy_book_message(45281.1, 45281.2);
        let result = feed.process(dummy_message);

        let Err(KrakenFeedError::ChecksumMismatch {
            computed: _,
            received: _,
        }) = result
        else {
            panic!("Expected to get checksum mismatch error, got {result:?} instead");
        };
    }
}
