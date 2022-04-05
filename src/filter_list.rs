use serde::{Deserialize, Serialize};

/// FilterList contains the information needed to process a single filter list
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FilterList {
    /// can be any string, must be unique among all filter lists
    pub id: String,
    /// source is the path to where to get the list from (probably a URL)
    pub source: String,
    /// tags describe the destinations where the processed URLs will end up
    pub tags: Vec<String>,
    /// regex to extract URL from a line
    pub regex: String,
}
