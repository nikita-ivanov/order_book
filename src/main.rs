use clap::Parser;

use anyhow::{Result, anyhow};
use kraken_l3::{
    kraken::feed::KrakenFeed,
    retry::{ExponentialRetryPolicy, NoRetryPolicy, RetryDecision, RetryPolicy, RetryPolicyKind},
    store::construct_file_stores,
};
use tokio::time::sleep;

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

    let stores = construct_file_stores(&args.symbols, args.depth);
    let mut feed = KrakenFeed::new(&args.symbols[..], args.depth as u16, true, stores).await?;

    loop {
        match feed.run_once().await {
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
