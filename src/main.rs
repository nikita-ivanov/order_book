use clap::Parser;

use anyhow::{Result, anyhow};
use futures_util::{
    SinkExt, StreamExt,
    stream::{SplitSink, SplitStream},
};
use kraken_l3::{
    kraken::feed::{KrakenFeed, KrakenFeedError},
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

    let mut feed = KrakenFeed::new(&args.symbols[..], args.depth as u16, true).await?;

    loop {
        match run_once(&mut feed).await {
            Ok(_) => continue,

            Err(e) => match retry_policy.on_failure() {
                RetryDecision::Wait(duration) => {
                    println!("Encountered error, will retry. Error was: {e:?}");
                    sleep(duration).await
                }

                RetryDecision::GiveUp => {
                    println!("{:?}", e);
                    return Err(anyhow!("Too many failures, exiting application"));
                }
            },
        }
    }
}

async fn run_once(feed: &mut KrakenFeed) -> Result<()> {
    let ws = connect().await?;
    let (mut tx, mut rx) = ws.split();

    request_channels(&mut tx, &feed.symbols, feed.depth as usize).await?;

    match receiving_loop(&mut rx, &mut *feed).await {
        Ok(()) => Ok(()),
        Err(other) => Err(anyhow!("received unexpected error during loop: {other:?}")),
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

async fn receiving_loop(rx: &mut Receiver, feed: &mut KrakenFeed) -> Result<()> {
    // TODO handle update/snapshot messages differently?
    while let Some(msg) = rx.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                eprintln!("Websocket error, will skip. Error was: {e:?}");
                continue;
            }
        };

        match feed.process(msg) {
            Ok(ob) => {
                // for (_, ob) in feed.order_books.iter() {
                //     ob.print();
                // }

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

            Err(KrakenFeedError::ChecksumMismatch { computed, received }) => {
                anyhow!(
                    "Checksum mismatch: received {received:?} vs. computed {computed:?}. Try to reconnect."
                )
            }

            Err(KrakenFeedError::PrecisionExceeded { precision, number }) => {
                anyhow!(
                    "Exchange sent a number with higher precision than expected, which is considered a breaking error.
                    Precision was {precision} while received number was {number}"
                )
            }

            Err(KrakenFeedError::OrderBookError { source }) => {
                anyhow!(
                    "Error during order book processing, try to reconnect. Error was\n{source:?}"
                )
            }
        };
    }

    Err(anyhow!("websocket exited, try to reconnect"))
}
