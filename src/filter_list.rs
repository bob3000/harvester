use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum TransformationName {
    Column,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Transformation {
    pub name: TransformationName,
    pub args: Vec<String>,
}

/// FilterList contains the information needed to process a single filter list
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FilterList {
    /// can be any string, must be unique among all filter lists
    pub id: String,
    /// source is the path to where to get the list from (probably a URL)
    pub source: String,
    /// tags describe the destinations where the processed URLs will end up
    pub tags: Vec<String>,
    /// functions to be applied to the data before writing it to it's destination
    pub transformations: Vec<Transformation>,
}
