#![feature(let_chains)]
mod config;
mod filter_controller;
mod filter_list;
mod input;
mod io;
mod output;
mod stages;

use std::{borrow::Cow, fmt::Display, path::Path, process::exit};

use clap::{Parser, ValueEnum};
use colored::*;
use env_logger::Env;
use filter_controller::{ChannelCommand, ChannelMessage, FilterController};
use flume::{Receiver, Sender};

use crate::config::Config;

#[macro_use]
extern crate log;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl From<&LogLevel> for Cow<'static, str> {
    fn from(value: &LogLevel) -> Self {
        return Cow::Owned(value.to_string());
    }
}

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

    let (cmd_tx, cmd_rx): (Sender<ChannelCommand>, Receiver<ChannelCommand>) = flume::unbounded();
    let (msg_tx, msg_rx): (Sender<ChannelMessage>, Receiver<ChannelMessage>) = flume::unbounded();

    // handle ctrl_c
    tokio::spawn(async move {
        debug!("received signal");
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
    let config = match Config::load(Path::new(&args.config)) {
        Err(e) => {
            error!("{}: {}", &args.config, e);
            exit(1);
        }
        Ok(c) => c,
    };

    // the lists are going through a process of four stages
    let mut download_controller = FilterController::new(config, cmd_rx, msg_tx);

    // start the processing chain by downloading the filter lists
    info!("{}", format!("Downalading lists ...").yellow());
    let mut extract_controller = match download_controller.run().await {
        Ok(c) => c,
        Err(e) => {
            error!("{:?}", e);
            exit(1);
        }
    };

    // the second stage extracts the URLs from the downloaded lists which come in heterogeneous formats
    info!("{}", format!("Extracting domains ...").yellow());
    let mut categorize_controller = match extract_controller.run().await {
        Ok(c) => c,
        Err(e) => {
            error!("{:?}", e);
            exit(1);
        }
    };

    // the third stage assembles the URLs into lists corresponding to the tags set in the configuration file
    info!("{}", format!("Categorizing domains ...").yellow());
    let mut output_controller = match categorize_controller.run().await {
        Ok(c) => c,
        Err(e) => {
            error!("{:?}", e);
            exit(1);
        }
    };

    // the fourth stage finally transforms the category lists into the desired output format
    info!("{}", format!("Creating output files ...").yellow());
    match output_controller.run().await {
        Ok(c) => c,
        Err(e) => {
            error!("{:?}", e);
            exit(1);
        }
    };
    Ok(())
}
