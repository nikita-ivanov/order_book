use anyhow::{Result, anyhow};
use std::sync::mpsc::Sender;
use std::sync::mpsc::channel;
use std::thread;
use std::thread::JoinHandle;

use crate::{book::snapshot::OrderBookSnapshot, store::OrderBookSink};

pub enum WriterCommand {
    Snapshot(OrderBookSnapshot),
    Finish,
}

/// BookWriter is responsible for saving orderer book shapshots. It runs in a separate OS thread.
/// BookWriter::shutdown() must be called on application exit to ensure all data is saved.
pub struct BookWriter {
    thread: JoinHandle<Result<()>>,
    sender: Sender<WriterCommand>,
}

impl BookWriter {
    pub fn new(mut store: Box<dyn OrderBookSink + Send>) -> BookWriter {
        let (sender, receiver) = channel::<WriterCommand>();

        let handle = thread::spawn(move || -> Result<()> {
            loop {
                let command = match receiver.recv() {
                    Ok(s) => s,
                    Err(_) => {
                        store.finish()?;
                        break;
                    }
                };

                match command {
                    WriterCommand::Snapshot(snapshot) => {
                        store.write(&snapshot).unwrap();
                    }
                    WriterCommand::Finish => {
                        store.finish()?;
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
