mod config;
mod filter_controller;
mod filter_list;
mod input;
mod io;
mod output;
mod stages;

use std::{path::Path, process::exit};

use env_logger::Env;
use filter_controller::{ChannelCommand, ChannelMessage, FilterController};
use flume::{Receiver, Sender};

use crate::config::Config;

#[macro_use]
extern crate log;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let env = Env::default()
        .filter_or("HV_LOG_LEVEL", "info")
        .write_style_or("HV_LOG_STYLE", "always");

    env_logger::init_from_env(env);

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
                    error!("{}", e);
                }
                ChannelMessage::Info(i) => {
                    info!("{}", i);
                }
                ChannelMessage::Debug(i) => {
                    debug!("{}", i);
                }
            }
        }
    });

    // crate configuration
    let config = match Config::load(Path::new("./config.json")) {
        Err(e) => {
            error!("{:?}", e);
            exit(1);
        }
        Ok(c) => c,
    };

    // the lists are going through a process of four stages
    let mut download_controller = FilterController::new(config, cmd_rx, msg_tx);

    // start the processing chain by downloading the filter lists
    info!("Downalading lists ...");
    let mut extract_controller = match download_controller.run().await {
        Ok(c) => c,
        Err(e) => {
            error!("{:?}", e);
            exit(1);
        }
    };

    // the second stage extracts the URLs from the downloaded lists which come in heterogeneous formats
    info!("Extracting domains ...");
    let mut categorize_controller = match extract_controller.run().await {
        Ok(c) => c,
        Err(e) => {
            error!("{:?}", e);
            exit(1);
        }
    };

    // the third stage assembles the URLs into lists corresponding to the tags set in the configuration file
    info!("Categorizing domains ...");
    let mut output_controller = match categorize_controller.run().await {
        Ok(c) => c,
        Err(e) => {
            error!("{:?}", e);
            exit(1);
        }
    };

    // the fourth stage finally transforms the category lists into the desired output format
    info!("Creating output files ...");
    match output_controller.run().await {
        Ok(c) => c,
        Err(e) => {
            error!("{:?}", e);
            exit(1);
        }
    };
    Ok(())
}
