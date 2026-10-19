use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use futures_util::StreamExt;
use std::collections::HashMap;
use std::sync::mpsc::Sender;
use tokio_tungstenite::tungstenite::Message;

use crate::book::snapshot::OrderBookSnapshot;
use crate::kraken::core::{Receiver, SymbolState, connect, request_channels};
use crate::kraken::error::KrakenFeedError;
use crate::{
    book::order_book::OrderBook,
    kraken::{
        models::{BasicInfo, BasicInfoResult, BookMessage},
        precision::get_precision,
    },
};

pub struct KrakenFeed {
    verify_checksum: bool,
    depth: u16,
    symbol_states: HashMap<String, SymbolState>,
}

impl KrakenFeed {
    // TODO It would probably make more sense to restrict one feed to one symbol?
    pub async fn new(
        symbols: &[String],
        depth: u16,
        verify_checksum: bool,
        // mut stores: HashMap<String, Box<dyn OrderBookSink>>,
        ob_writer: Sender<OrderBookSnapshot>,
    ) -> Result<KrakenFeed> {
        let mut precisions = get_precision(symbols).await?;

        let mut states: HashMap<String, SymbolState> = HashMap::new();

        for symbol in symbols {
            let precision = precisions.remove(symbol).unwrap();

            let ob = OrderBook::new(
                String::from(symbol),
                depth,
                precision.pair_decimals,
                precision.lot_decimals,
                Utc::now(),
            );

            let state = SymbolState::new(ob, precision, ob_writer.clone());

            states.insert(symbol.clone(), state);
        }

        Ok(KrakenFeed {
            verify_checksum,
            depth,
            symbol_states: states,
        })
    }

    fn process(&mut self, message: Message) -> Result<&SymbolState, KrakenFeedError> {
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

        let book_data = &message.data[0];

        let symbol = &book_data.symbol;
        let symbol_state = match self.symbol_states.get_mut(symbol) {
            Some(state) => state,
            None => {
                return Err(KrakenFeedError::OtherError {
                    info: format!("Missing symbol state for {symbol}"),
                });
            }
        };

        symbol_state.apply_update(book_data, self.verify_checksum)?;

        // TODO this is probably not ok
        Ok(symbol_state)
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
                Ok(state) => {
                    state.order_book.get_snapshot().print();

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

                Err(e @ KrakenFeedError::ChecksumMismatch { .. }) => {
                    return Err(e.into());
                }

                Err(e @ KrakenFeedError::PrecisionExceeded { .. }) => {
                    return Err(e.into());
                }

                Err(e @ KrakenFeedError::OrderBookError { .. }) => {
                    return Err(e).context("Error during order book processing, try to reconnect.");
                }

                Err(e @ KrakenFeedError::StoreError { .. }) => {
                    return Err(e).context("Error saving order book");
                }

                Err(e @ KrakenFeedError::OtherError { .. }) => return Err(e.into()),
            };
        }

        Err(anyhow!("websocket exited, try to reconnect"))
    }

    pub async fn run_once(&mut self) -> Result<()> {
        let ws = connect().await?;
        let (mut tx, mut rx) = ws.split();

        let symbols: Vec<String> = self.symbol_states.keys().map(String::from).collect();

        request_channels(&mut tx, &symbols, self.depth as usize).await?;

        match self.receiving_loop(&mut rx).await {
            Ok(()) => Ok(()),
            Err(other) => Err(anyhow!("received unexpected error during loop: {other:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use std::{
        collections::HashMap,
        sync::mpsc::{Receiver, channel},
    };
    use tokio_tungstenite::tungstenite::{Bytes, Message, Utf8Bytes};

    use crate::{
        book::{order_book::OrderBook, snapshot::OrderBookSnapshot},
        kraken::{
            core::SymbolState,
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

    fn get_feed(depth: u16) -> (KrakenFeed, Receiver<OrderBookSnapshot>) {
        let ob = OrderBook::new("dummy order book".to_string(), depth, 1, 2, Utc::now());

        let (tx, rx) = channel::<OrderBookSnapshot>();

        let symbol_state = SymbolState::new(
            ob,
            AssetPairPrecision {
                pair_decimals: 1,
                lot_decimals: 8,
            },
            tx,
        );

        (
            KrakenFeed {
                depth,
                verify_checksum: true,
                symbol_states: HashMap::from([("BTC/USD".to_string(), symbol_state)]),
            },
            rx,
        )
    }

    #[test]
    fn test_full_message_prcessing_passes_checksum() {
        let book_message = get_book_message();

        let (mut feed, _rx) = get_feed(10);

        // this will verify checksum
        feed.process(book_message).unwrap();
    }

    #[test]
    fn test_checksum_ignores_orders_beyond_10th() {
        let (mut feed, _rx) = get_feed(20);

        let real_message = get_book_message();
        feed.process(real_message).unwrap();

        // this should push (low bid and high ask) levels beyond 10th and ignore them for checksum
        let dummy_message = get_dummy_book_message(100.0, 100_000.0);
        feed.process(dummy_message).unwrap();
    }

    #[test]
    fn test_checksum_mismatch_returns_error() {
        let (mut feed, _rx) = get_feed(20);

        let real_message = get_book_message();
        feed.process(real_message).unwrap();

        let (mut feed, _rx) = get_feed(20);

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
            panic!("Expected to get checksum mismatch error");
        };
    }
}
