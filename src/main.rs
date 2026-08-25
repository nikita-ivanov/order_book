use clap::Parser;
use std::collections::HashMap;

use anyhow::{Result, anyhow};
use futures_util::{
    SinkExt, StreamExt,
    stream::{SplitSink, SplitStream},
};
use kraken_l3::{
    BasicInfo, BasicInfoResult, BookMessage,
    book::order_book::{OrderBook, OrderBookError},
    precision::get_precision,
    retry::{ExponentialRetryPolicy, NoRetryPolicy, RetryDecision, RetryPolicy, RetryPolicyKind},
};
use tokio::{net::TcpStream, time::sleep};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};

/// Order book streaming from Kraken WS (v2).
#[derive(Parser, Debug)]
struct Args {
    /// Vector of pairs to collect order book for
    #[arg(required = true, num_args = 1)]
    symbols: Vec<String>,

    /// Order book depth
    #[arg(short, long, default_value_t = 10)]
    depth: usize,

    #[arg(value_enum, short, long, default_value_t = RetryPolicyKind::Exponential)]
    retry_policy: RetryPolicyKind,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let mut retry_policy: Box<dyn RetryPolicy> = match args.retry_policy {
        RetryPolicyKind::Exponential => Box::new(ExponentialRetryPolicy::default()),
        RetryPolicyKind::NoRetry => Box::new(NoRetryPolicy {}),
    };

    loop {
        match run_once(&args).await {
            Ok(_) => {
                println!("websocket exited cleanly, reconnecting...");
                continue;
            }
            Err(_) => match retry_policy.on_failure() {
                RetryDecision::Wait(duration) => sleep(duration).await,
                RetryDecision::GiveUp => {
                    return Err(anyhow!("Too many failures, exiting application"));
                }
            },
        }
    }
}

async fn run_once(args: &Args) -> Result<()> {
    let ws = connect().await?;
    let (mut tx, mut rx) = ws.split();
    let precisions = get_precision(&args.symbols).await?;

    let mut order_books = HashMap::new();
    for (symbol, precision) in precisions.iter() {
        let ob = OrderBook::new(
            args.depth,
            precision.pair_decimals,
            precision.lot_decimals,
            true,
        );
        order_books.insert(symbol, ob);
    }

    request_channels(&mut tx, &args.symbols, args.depth).await?;

    match receiving_loop(&mut rx, &mut order_books).await {
        Ok(_) => Ok(()),
        Err(err) => match err {
            OrderBookError::PrecisionExceeded { precision, number } => {
                panic!(
                    "Exchange sent a number with higher precision than expected, which is considered a breaking error.
                     Precision was {precision} while received number was {number}");
            }
            other => return Err(anyhow!("received error during loop: {other:?}")),
        },
    }
}

type WebsocketStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type Transmitter = SplitSink<WebsocketStream, Message>;
type Receiver = SplitStream<WebsocketStream>;

async fn connect() -> Result<WebsocketStream> {
    let url = "wss://ws.kraken.com/v2";
    let (ws, _) = connect_async(url).await?;

    Ok(ws)
}

async fn request_channels(tx: &mut Transmitter, symbols: &Vec<String>, depth: usize) -> Result<()> {
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

async fn receiving_loop(
    rx: &mut Receiver,
    order_books: &mut HashMap<&String, OrderBook>,
) -> Result<(), OrderBookError> {
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

        let symbol = &message.data[0].symbol;
        let order_book = match order_books.get_mut(symbol) {
            Some(book) => book,
            None => panic!("could not find order book "),
        };

        order_book.apply_update(&message)?;
        println!("{:?}", order_book.top_of_book());
    }
    Ok(())
}
