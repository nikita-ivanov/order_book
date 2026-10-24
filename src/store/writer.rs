use anyhow::{Result, anyhow};
use std::sync::mpsc::Sender;
use std::thread;
use std::thread::JoinHandle;
use std::{collections::HashMap, sync::mpsc::channel};

use crate::{book::snapshot::OrderBookSnapshot, store::OrderBookSink};

pub enum WriterCommand {
    Snapshot(OrderBookSnapshot),
    Finish,
}

fn finish_stores(mut stores: HashMap<String, Box<dyn OrderBookSink + Send>>) -> Result<()> {
    let mut errors = Vec::new();

    for (store_name, store) in stores.iter_mut() {
        let _ = store.finish().map_err(|e| errors.push((store_name, e)));
    }

    if errors.is_empty() {
        Ok(())
    } else {
        let num_errors = errors.len();
        let total_stores = stores.len();

        Err(anyhow!(format!(
            "failed to finish {num_errors}/{total_stores} stores"
        )))
    }
}

/// BookWriter is responsible for saving orderer book shapshots. It runs in a separate OS thread.
/// BookWriter::shutdown() must be called on application exit to ensure all data is saved.
pub struct BookWriter {
    thread: JoinHandle<Result<()>>,
    sender: Sender<WriterCommand>,
}

impl BookWriter {
    pub fn new(mut stores: HashMap<String, Box<dyn OrderBookSink + Send>>) -> BookWriter {
        let (sender, receiver) = channel::<WriterCommand>();

        let handle = thread::spawn(move || -> Result<()> {
            loop {
                let command = match receiver.recv() {
                    Ok(s) => s,
                    Err(_) => {
                        finish_stores(stores)?;
                        break;
                    }
                };

                match command {
                    WriterCommand::Snapshot(snapshot) => {
                        let snapshot_name = &snapshot.name;

                        let store = match stores.get_mut(snapshot_name) {
                            Some(s) => s,
                            None => {
                                // TODO add custom errors?
                                return Err(anyhow!(format!(
                                    "Could not find store for snapshot {snapshot_name}, will skip"
                                )));
                            }
                        };

                        store.write(&snapshot).unwrap();
                    }
                    WriterCommand::Finish => {
                        finish_stores(stores)?;
                        break;
                    }
                }
            }

            Ok(())
        });

        BookWriter {
            thread: handle,
            sender,
        }
    }

    pub fn sender(&self) -> Sender<WriterCommand> {
        self.sender.clone()
    }

    pub fn shutdown(self) -> Result<()> {
        self.sender.send(WriterCommand::Finish)?;

        match self.thread.join() {
            Ok(thread_result) => match thread_result {
                Ok(()) => Ok(()),
                Err(e) => Err(e),
            },
            Err(_) => Err(anyhow!("failed to join order book sink thread")),
        }
    }
}
