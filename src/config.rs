use std::io::prelude::*;
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::{filter_list::FilterList, output::OutputType};

pub const CACHED_CONF_FILE_NAME: &str = "last_conf.json";

/// Config contains all relevant information to start the data processing.
/// Relevant information is considered most of all data sources and destinations
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub lists: Vec<FilterList>,
    pub cache_dir: String,
    pub output_dir: String,
    pub output_format: OutputType,
    pub cached_config: Option<Box<Self>>,
}

impl Config {
    /// Populates the Config struct from a json file
    ///
    /// * `path`: file system path the the configuration file
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let contents = fs::read_to_string(path).with_context(|| "error reading config file")?;
        let mut config: Config = serde_json::from_str(&contents).with_context(|| "invalid json")?;

        // just do one recursion
        if path.ends_with(CACHED_CONF_FILE_NAME) {
            return Ok(config);
        }

        // load cached config if available
        let cached_config_path =
            PathBuf::from(format!("{}/{}", config.cache_dir, CACHED_CONF_FILE_NAME));
        if let Ok(c) = Config::load(&cached_config_path) {
            debug!("found cached config");
            config.cached_config = Some(Box::new(c));
        } else {
            debug!("no cached config found");
        }

        Ok(config)
    }

    /// write used config to the cache folder for use on next run
    pub fn save_to_cache(&mut self) -> anyhow::Result<()> {
        // don't grow recursively
        self.cached_config = None;
        let mut last_conf_path = PathBuf::from(&self.cache_dir);
        last_conf_path.push(CACHED_CONF_FILE_NAME);
        let mut last_conf = File::create(&last_conf_path)?;
        let conf_str = serde_json::to_string(&self)?;
        last_conf.write_all(&conf_str.as_bytes())?;
        Ok(())
    }

    /// extracts all existing tags from the filter list configuration
    pub fn get_tags(&self) -> Vec<String> {
        let mut tags: Vec<String> = Vec::new();
        for list in self.lists.iter() {
            list.tags.iter().for_each(|t| {
                if !tags.contains(t) {
                    tags.push(t.clone())
                }
            });
        }
        tags
    }

    pub fn lists_with_tag(&self, tag: &String) -> Vec<&FilterList> {
        let lists: Vec<&FilterList> = self.lists.iter().filter(|l| l.tags.contains(tag)).collect();
        lists
    }
}
