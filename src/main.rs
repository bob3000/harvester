mod config;
mod filter_controller;
mod filter_list;
mod filter_list_io;
mod input;

use std::{path::Path, process::exit};

use filter_controller::{ChannelCommand, ChannelMessage, FilterController};
use flume::{Receiver, Sender};

use crate::config::Config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let verbose = true;
    let debug = true;
    let (cmd_tx, cmd_rx): (Sender<ChannelCommand>, Receiver<ChannelCommand>) = flume::unbounded();
    let (msg_tx, msg_rx): (Sender<ChannelMessage>, Receiver<ChannelMessage>) = flume::unbounded();

    // handle ctrl_c
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.unwrap();
        cmd_tx.send(ChannelCommand::Quit).unwrap();
    });

    // handle messages from channels
    tokio::spawn(async move {
        while let Ok(msg) = msg_rx.recv() {
            match msg {
                ChannelMessage::Error(e) => {
                    eprintln!("{:?}", e);
                }
                ChannelMessage::Info(i) => {
                    if verbose {
                        println!("{}", i);
                    }
                }
                ChannelMessage::Debug(i) => {
                    if debug {
                        println!("{}", i);
                    }
                }
            }
        }
    });

    let config = match Config::load(Path::new("./config.json")) {
        Err(e) => {
            println!("{:?}", e);
            exit(1);
        }
        Ok(c) => c,
    };
    let mut controller = FilterController::new(config, cmd_rx, msg_tx);
    if let Err(e) = controller.run().await {
        eprintln!("{}", e)
    }
    Ok(())
}
