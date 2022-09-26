#![feature(let_chains)]
mod config;
mod filter_controller;
mod filter_list;
mod input;
mod io;
mod output;
mod stages;

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

    // crate configuration
    let config = match Config::load(Path::new("./config.json")) {
        Err(e) => {
            println!("{:?}", e);
            exit(1);
        }
        Ok(c) => c,
    };

    // the lists are going through a process of four stages
    let mut download_controller = FilterController::new(config, cmd_rx, msg_tx);

    // start the processing chain by downloading the filter lists
    let mut extract_controller = match download_controller.run().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{:?}", e);
            exit(1);
        }
    };

    // the second stage extracts the URLs from the downloaded lists which come in heterogeneous formats
    let mut categorize_controller = match extract_controller.run().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{:?}", e);
            exit(1);
        }
    };

    // the third stage assembles the URLs into lists corresponding to the tags set in the configuration file
    let mut output_controller = match categorize_controller.run().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{:?}", e);
            exit(1);
        }
    };

    // the fourth stage finally transforms the category lists into the desired output format
    match output_controller.run().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{:?}", e);
            exit(1);
        }
    };
    Ok(())
}
