use anyhow::Result;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
struct AssetPairResponse {
    error: Vec<String>,
    result: HashMap<String, AssetPairPrecision>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AssetPairPrecision {
    pub pair_decimals: usize,
    pub lot_decimals: usize,
}

pub async fn get_precision(pairs: &Vec<String>) -> Result<HashMap<String, AssetPairPrecision>> {
    let url = "https://api.kraken.com/0/public/AssetPairs?aclass_base=currency&info=info&execution_venue=international&assetVersion=1";
    let pairs = pairs.join(",");
    let params = [("pair", pairs)];

    let url = reqwest::Url::parse_with_params(url, &params)
        .unwrap()
        .to_string();

    let response = reqwest::get(url).await.unwrap();
    let result = response.text().await.unwrap();
    let result_parsed: AssetPairResponse = serde_json::from_str(&result[..]).unwrap();

    Ok(result_parsed.result)
}
