use std::{fs, path::Path};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::{filter_list::FilterList, output::OutputType};

/// Config contains all relevant information to start the data processing.
/// Relevant information is considered most of all data sources and destinations
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub lists: Vec<FilterList>,
    pub tmp_dir: String,
    pub out_dir: String,
    pub out_format: OutputType,
}

impl Config {
    /// Populates the Config struct from a json file
    ///
    /// * `path`: file system path the the configuration file
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let contents = fs::read_to_string(path).with_context(|| "error reading config file")?;
        let config: Config = serde_json::from_str(&contents).with_context(|| "invalid json")?;
        let Self {
            lists,
            tmp_dir,
            out_dir,
            out_format,
        } = config;
        Ok(Self {
            lists,
            tmp_dir,
            out_dir,
            out_format,
        })
    }
}
