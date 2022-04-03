use std::{fs, path::Path};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::filter_list::FilterList;

/// Config contains all relevant information to start the data processing.
/// Relevant information is considered most of all data sources and destinations
#[derive(Deserialize, Serialize)]
pub(crate) struct Config {
    lists: Vec<FilterList>,
}

impl Config {
    /// Populates the Config struct from a json file
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let contents = fs::read_to_string(path)?;
        let config: Config =
            serde_json::from_str(&contents).with_context(|| "could not read configuration file")?;
        Ok(Self {
            lists: config.lists,
        })
    }

    pub fn lists(&self) -> &Vec<FilterList> {
        &self.lists
    }
}
