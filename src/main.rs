mod config;
mod filter_controller;
mod filter_list;
mod input;

use std::path::Path;

use filter_controller::FilterController;

use crate::config::Config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::load(Path::new("config.json"))?;
    let controller = FilterController::new(config.lists());
    controller.run().await?;
    Ok(())
}
