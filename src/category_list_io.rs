use std::{io::Write, sync::Arc};

use futures::lock::Mutex;

use crate::input::Input;

/// CategoryListIO contains a reader and a writer used to manipulate category
/// wise assembled filter lists
#[derive(Debug)]
pub struct CategoryListIO<R: Input + Send + Sync, W: Write + Send + Sync> {
    pub name: String,
    pub reader: Option<Arc<Mutex<R>>>,
    pub writer: Option<Arc<Mutex<W>>>,
}

impl<R: Input + Send + Sync, W: Write + Send + Sync> CategoryListIO<R, W> {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            reader: None,
            writer: None,
        }
    }
}
