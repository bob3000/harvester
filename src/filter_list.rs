use serde::{Deserialize, Serialize};

/// FilterList contains the information needed to process a single filter list
#[derive(Debug, Deserialize, Serialize)]
pub struct FilterList {
    /// source is the path to where to get the list from (probably a URL)
    pub source: String,
    /// tags describe the destinations where the processed URLs will end up
    pub tags: Vec<String>,
    /// functions to be applied to the data before writing it to it's destination
    pub transformations: Vec<String>,
}
