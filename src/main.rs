use anyhow::Result;
use clap::Parser;
use kraken_l3::{
    kraken::feed::KrakenFeed,
    retry::{ExponentialRetryPolicy, NoRetryPolicy, RetryDecision, RetryPolicy, RetryPolicyKind},
    store::{construct_file_stores, writer::BookWriter},
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

// #[tokio::main]
// async fn main2() -> Result<()> {
//     let path = Path::new("/Users/nivanov/ob_data/eth_usd.parquet");
//     let file = OpenOptions::new().read(true).open(path).unwrap();
//     let reader = ParquetRecordBatchReader::try_new(file, 1).unwrap();
//
//     for chunk in reader {
//         println!("{chunk:?}");
//     }
//
//     Ok(())
// }

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let mut retry_policy: Box<dyn RetryPolicy> = match args.retry_policy {
        RetryPolicyKind::Exponential => Box::new(ExponentialRetryPolicy::default()),
        RetryPolicyKind::NoRetry => Box::new(NoRetryPolicy {}),
    };

    let stores = construct_file_stores(&args.symbols, args.depth);
    let book_writer = BookWriter::new(stores);
    let mut feed =
        KrakenFeed::new(&args.symbols, args.depth as u16, true, book_writer.sender()).await?;

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

    book_writer.shutdown();

    Ok(())
}
