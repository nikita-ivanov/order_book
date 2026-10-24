use std::sync::mpsc::Sender;

use anyhow::Result;
use futures_util::{
    SinkExt,
    stream::{SplitSink, SplitStream},
};
use serde_json::Number;
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};

use crate::{
    book::{
        order_book::{BookUpdate, Level, OrderBook},
        snapshot::OrderBookSnapshot,
    },
    kraken::{error::KrakenFeedError, models::BookData, precision::AssetPairPrecision},
    store::writer::WriterCommand,
};

type WebsocketStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type Transmitter = SplitSink<WebsocketStream, Message>;
pub type Receiver = SplitStream<WebsocketStream>;

pub async fn request_channels(tx: &mut Transmitter, symbols: &[&str], depth: usize) -> Result<()> {
    let data = serde_json::json!({
        "method": "subscribe",
        "params": {
            "channel": "book",
            "depth": depth,
            "symbol": symbols,
        }
    });

    tx.send(Message::text(data.to_string())).await?;

    Ok(())
}

pub async fn connect() -> Result<WebsocketStream> {
    let url = "wss://ws.kraken.com/v2";
    let (ws, _) = connect_async(url).await?;

    Ok(ws)
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

pub struct SymbolState {
    pub name: String, // TODO is it better to use &str here?
    pub order_book: OrderBook,
    pub precision: AssetPairPrecision,
    pub writer: Sender<WriterCommand>,
}

// TODO should have method to return order book snapshot
impl SymbolState {
    pub fn new(
        name: &str,
        order_book: OrderBook,
        precision: AssetPairPrecision,
        // store: Box<dyn OrderBookSink>,
        writer: Sender<WriterCommand>,
    ) -> SymbolState {
        SymbolState {
            name: name.to_string(),
            order_book,
            precision,
            writer,
        }
    }
    pub fn apply_update(
        &mut self,
        book_data: &BookData,
        verify_checksum: bool,
    ) -> Result<OrderBookSnapshot, KrakenFeedError> {
        let mut bids = Vec::<Level>::new();
        for bid in &book_data.bids {
            let price = number_to_i64(&bid.price, self.precision.pair_decimals)?;
            let quantity = number_to_i64(&bid.qty, self.precision.lot_decimals)?;
            bids.push(Level { price, quantity })
        }

        let mut asks = Vec::<Level>::new();
        for ask in &book_data.asks {
            let price = number_to_i64(&ask.price, self.precision.pair_decimals)?;
            let quantity = number_to_i64(&ask.qty, self.precision.lot_decimals)?;
            asks.push(Level { price, quantity })
        }

        let book_update = BookUpdate {
            bids: Some(bids),
            asks: Some(asks),
            timestamp: book_data.timestamp,
        };

        self.order_book
            .apply_update(book_update)
            .map_err(|e| KrakenFeedError::OrderBookError { source: e })?;

        if verify_checksum {
            let bids: Vec<(i64, i64)> = self
                .order_book
                .bids
                .iter()
                .map(|pair| (*pair.0, *pair.1))
                .collect();

            let asks: Vec<(i64, i64)> = self
                .order_book
                .asks
                .iter()
                .map(|pair| (*pair.0, *pair.1))
                .collect();

            let computed_checksum = compute_checksum(&bids, &asks);

            let received_checksum = book_data.checksum;

            if computed_checksum != received_checksum {
                return Err(KrakenFeedError::ChecksumMismatch {
                    computed: computed_checksum,
                    received: received_checksum,
                });
            }
        };

        self.writer
            .send(WriterCommand::Snapshot(self.order_book.get_snapshot()))
            .map_err(|e| KrakenFeedError::StoreError { source: e.into() })?;

        Ok(self.order_book.get_snapshot())
    }
}
