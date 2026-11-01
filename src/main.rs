use anyhow::Result;
use clap::Parser;
use kraken_l3::{
    kraken::feed::KrakenFeed,
    retry::{ExponentialRetryPolicy, NoRetryPolicy, RetryDecision, RetryPolicy, RetryPolicyKind},
    store::{OrderBookStoreKind, construct_file_store, writer::BookWriter},
};
use tokio::time::sleep;

#[derive(Parser, Debug)]
struct Args {
    #[arg(required = true, num_args = 1)]
    symbol: String,

    #[arg(short, long, default_value_t = 10)]
    depth: usize,

    #[arg(short, long, default_value_t = OrderBookStoreKind::ParquetLong)]
    store: OrderBookStoreKind,

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

    let store = construct_file_store(&args.symbol, args.depth, args.store);
    let book_writer = BookWriter::new(store);
    let mut feed =
        KrakenFeed::new(&args.symbol, args.depth as u16, true, book_writer.sender()).await?;

    loop {
        tokio::select! {
            result = feed.run_once() => {
                match result {
                    Ok(_) => continue,

                    Err(e) => match retry_policy.on_failure() {
                        RetryDecision::Wait(duration) => {
                            println!("Encountered error, will retry. Error was: {e:?}");
                            sleep(duration).await
                        }

                        RetryDecision::GiveUp => {
                            println!("{:?}", e);
                            println!("too many errors, giving up and exiting the application");
                            break;
                            // return Err(anyhow!("Too many failures, exiting application"));
                        }
                    }
                }
            }
            // on ctrl c shutdowns, break the loop such thtat all Drop traits can run to cleanup
            _ = tokio::signal::ctrl_c() => {
                    println!("received ctrl c, gracefull shutdown");
                    break;
                }
        };
    }

    match book_writer.shutdown() {
        Ok(()) => Ok(()),
        Err(e) => {
            println!(
                "Error during book writer shutdown. If hangs, kill the process. Error was:\n{e:?}"
            );
            Ok(())
        }
    }
}
