use anyhow::Result;
use futures_util::{
    SinkExt,
    stream::{SplitSink, SplitStream},
};
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};

type WebsocketStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type Transmitter = SplitSink<WebsocketStream, Message>;
pub type Receiver = SplitStream<WebsocketStream>;

pub async fn request_channels(
    tx: &mut Transmitter,
    symbols: &Vec<String>,
    depth: usize,
) -> Result<()> {
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
