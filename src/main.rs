use anyhow::Result;
use futures_util::{
    SinkExt, StreamExt,
    stream::{SplitSink, SplitStream},
};
use kraken_l3::{BasicInfo, BasicInfoResult, BookMessage, book::order_book::OrderBook};
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};

#[tokio::main]
async fn main() -> Result<()> {
    let ws = connect().await?;
    let (mut tx, mut rx) = ws.split();

    let symbols = vec![String::from("ETH/USD")];
    let depth = 25;

    let mut order_book = OrderBook::new(depth, 2, 8);

    request_channels(&mut tx, symbols, depth).await?;
    receiving_loop(&mut rx, &mut order_book).await?;

    Ok(())
}

type WebsocketStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type Transmitter = SplitSink<WebsocketStream, Message>;
type Receiver = SplitStream<WebsocketStream>;

async fn connect() -> Result<WebsocketStream> {
    let url = "wss://ws.kraken.com/v2";
    let (ws, _) = connect_async(url).await?;

    Ok(ws)
}

async fn request_channels(tx: &mut Transmitter, symbols: Vec<String>, depth: usize) -> Result<()> {
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

async fn receiving_loop(rx: &mut Receiver, order_book: &mut OrderBook) -> Result<()> {
    // TODO build a reconnect loop
    // TODO handle update/snapshot messages differently?
    while let Some(msg) = rx.next().await {
        // println!("{msg:?}");
        let message: BookMessage = match msg {
            Ok(Message::Text(bytes)) => {
                let info: BasicInfo = match serde_json::from_str(&bytes) {
                    Ok(result) => result,
                    Err(e) => {
                        println!("Error parsing basic info, skipping");
                        println!("Message was: {bytes:?}");
                        println!("Errors was: {e:?}");
                        continue;
                    }
                };

                let msg_type = info.parse();
                match msg_type {
                    BasicInfoResult::Snapshot | BasicInfoResult::Update => {
                        match serde_json::from_str(&bytes) {
                            Ok(result) => result,
                            Err(e) => {
                                println!("Error parsing message, will continue");
                                println!("Message was: {bytes:?}");
                                println!("Error was: {e:?}");
                                continue;
                            }
                        }
                    }
                    _ => {
                        println!("Skipping message as it does not have order book data");
                        println!("{bytes:?}");
                        println!("{}", "-".repeat(50));
                        continue;
                    }
                }
            }
            Ok(other) => {
                println!("Ignoring message: {other:?}");
                println!("{}", "-".repeat(50));
                continue;
            }
            Err(e) => {
                println!("Websocket error, will skip. Error was {e:?}");
                continue;
            }
        };

        order_book.apply_update(&message);
        println!("{:?}", order_book.top_of_book());
    }
    Ok(())
}
