#![feature(let_chains)]
mod config;
mod filter_controller;
mod filter_list;
mod input;
mod io;
mod log_level;
mod output;
mod stages;
mod tests;

use std::{
    path::Path,
    process::exit,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use clap::Parser;
use colored::*;
use env_logger::Env;
use filter_controller::{ChannelMessage, FilterController};
use flume::{Receiver, Sender};
use log_level::LogLevel;

use crate::config::Config;

#[macro_use]
extern crate log;

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    #[arg(short, long, default_value = "config.json")]
    config: String,
    #[arg(value_enum, short, long, default_value = "warn")]
    log_level: LogLevel,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // setup command line interface
    let args = Args::parse();

    // initialize logging
    let env = Env::default()
        .filter_or("HV_LOG_LEVEL", &args.log_level)
        .write_style_or("HV_LOG_STYLE", "auto");

    let mut builder = env_logger::Builder::from_env(env);
    builder.format_timestamp(None).format_target(false).init();

    // setup message channel to enable log output from tokio tasks
    let (msg_tx, msg_rx): (Sender<ChannelMessage>, Receiver<ChannelMessage>) = flume::unbounded();
    let message_rx = msg_rx.clone();

    // is_processing determines if the program was interrupted or is still running
    let is_processing = Arc::new(AtomicBool::new(true));
    let is_proc = Arc::clone(&is_processing);

    // handle ctrl_c
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.unwrap();
        info!("{}", "gracefully shutting down ...".yellow());
        is_proc.store(false, Ordering::SeqCst);
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
                ChannelMessage::Shutdown => {
                    debug!("message channel received Shutdown request");
                    break;
                }
                ChannelMessage::Warn(i) => {
                    warn!("{}", i);
                }
            }
        }
        // wait for channel to shutdown
        let _ = msg_rx.recv();
    });

    // crate configuration
    let config = match Config::load(Path::new(&args.config)) {
        Err(e) => {
            error!("{}: {}", &args.config, e);
            exit(1);
        }
        Ok(c) => c,
    };

    // the lists are going through a process of four stages
    let mut download_controller =
        FilterController::new(config, msg_tx.clone(), is_processing.clone());

    // start the processing chain by downloading the filter lists
    info!("{}", "Downalading lists ...".yellow());
    let mut extract_controller = match download_controller.run().await {
        Ok(c) => c,
        Err(e) => {
            error!("{:?}", e);
            exit(1);
        }
    };

    // the second stage extracts the URLs from the downloaded lists which come in heterogeneous formats
    if is_processing.load(Ordering::SeqCst) {
        info!("{}", "Extracting domains ...".yellow());
    }
    let mut categorize_controller = match extract_controller.run().await {
        Ok(c) => c,
        Err(e) => {
            error!("{:?}", e);
            exit(1);
        }
    };

    // the third stage assembles the URLs into lists corresponding to the tags set in the configuration file
    if is_processing.load(Ordering::SeqCst) {
        info!("{}", "Categorizing domains ...".yellow());
    }
    let mut output_controller = match categorize_controller.run().await {
        Ok(c) => c,
        Err(e) => {
            error!("{:?}", e);
            exit(1);
        }
    };

    // the fourth stage finally transforms the category lists into the desired output format
    if is_processing.load(Ordering::SeqCst) {
        info!("{}", "Creating output files ...".yellow());
    }
    match output_controller.run().await {
        Ok(c) => c,
        Err(e) => {
            error!("{:?}", e);
            exit(1);
        }
    };

    msg_tx.send(ChannelMessage::Shutdown).unwrap_or_else(|m| {
        debug!("filter_controller: {}", m);
    });

    // drain message channel
    for msg in message_rx.drain() {
        match msg {
            ChannelMessage::Debug(i) => debug!("{}", i),
            ChannelMessage::Error(i) => error!("{}", i),
            ChannelMessage::Info(i) => info!("{}", i),
            ChannelMessage::Warn(i) => warn!("{}", i),
            ChannelMessage::Shutdown => {}
        }
    }
    Ok(())
}
