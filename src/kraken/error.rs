use std::num::ParseIntError;

use thiserror::Error;
use tokio_tungstenite::tungstenite::Message;

use crate::book::error::OrderBookError;

#[derive(Debug, Error)]
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

    #[error("Failed to saved order book: {source:?}")]
    StoreError {
        #[source]
        source: anyhow::Error,
    },

    #[error("Other error")]
    OtherError { info: String },
}
