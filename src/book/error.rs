use std::num::ParseIntError;

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
