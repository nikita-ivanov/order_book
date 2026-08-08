use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Kraken has inconsistent response shape where messages do not share the same schema.
/// We use small strcut below to decide what to do with the message.
#[derive(Serialize, Deserialize, Debug)]
pub struct BasicInfo {
    pub channel: Option<String>,
    pub r#type: Option<String>,
}

pub enum BasicInfoResult {
    Update,
    Snapshot,
    Unknown,
}

impl BasicInfo {
    pub fn parse(self) -> BasicInfoResult {
        let is_book_data = self.channel.is_some_and(|c| c.contains("book"));

        match self.r#type {
            Some(t) => {
                if is_book_data && t.contains("update") {
                    BasicInfoResult::Update
                } else if is_book_data && t.contains("snapshot") {
                    BasicInfoResult::Snapshot
                } else {
                    BasicInfoResult::Unknown
                }
            }
            None => BasicInfoResult::Unknown,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BookMessage {
    channel: String,
    r#type: String,
    data: [BookData; 1],
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BookData {
    symbol: String,
    bids: Vec<Quote>,
    asks: Vec<Quote>,
    checksum: u32,
    timestamp: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Quote {
    price: f64,
    qty: f64,
}

// below do are not really needed since they are essentially the same models
#[derive(Serialize, Deserialize, Debug)]
pub struct BookSnapshot {
    channel: String,
    r#type: String,
    symbol: String,
    bids: Vec<Quote>,
    asks: Vec<Quote>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BookUpdate {
    channel: String,
    r#type: String,
    symbol: String,
    bid: Quote,
    ask: Quote,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum BookMessageV2 {
    Snapshot(BookSnapshot),
    Update(BookUpdate),
}
