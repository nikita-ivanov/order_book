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
    pub channel: String,
    pub r#type: String,
    pub data: [BookData; 1],
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BookData {
    pub symbol: String,
    pub bids: Vec<Quote>,
    pub asks: Vec<Quote>,
    pub checksum: u32,
    pub timestamp: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Quote {
    pub price: f64,
    pub qty: f64,
}

// below do are not really needed since they are essentially the same models
// #[derive(Serialize, Deserialize, Debug)]
// pub struct BookSnapshot {
//     channel: String,
//     r#type: String,
//     symbol: String,
//     bids: Vec<Quote>,
//     asks: Vec<Quote>,
// }

// #[derive(Serialize, Deserialize, Debug)]
// pub struct BookUpdate {
//     channel: String,
//     r#type: String,
//     symbol: String,
//     bid: Quote,
//     ask: Quote,
// }

// #[derive(Serialize, Deserialize, Debug)]
// pub enum BookMessageV2 {
//     Snapshot(BookSnapshot),
//     Update(BookUpdate),
// }
