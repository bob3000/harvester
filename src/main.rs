mod config;
mod filter_controller;
mod filter_list;
mod input;

use std::{path::Path, process::exit};

use filter_controller::FilterController;

use crate::config::Config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = match Config::load(Path::new("config.json")) {
        Err(e) => {
            println!("{}", e);
            exit(1);
        }
        Ok(c) => c,
    };
    let controller = FilterController::new(config);
    if let Err(e) = controller.run().await {
        eprintln!("{}", e)
    }
    Ok(())
}
