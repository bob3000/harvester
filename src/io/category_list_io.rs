use std::{io::Write, sync::Arc};

use futures::lock::Mutex;

use crate::input::Input;

/// CategoryListIO contains a reader and a writer used to manipulate category wise
/// assembled filter lists
#[derive(Debug)]
pub struct CategoryListIO<R: Input + Send, W: Write + Send> {
    pub name: String,
    pub reader: Option<Arc<Mutex<R>>>,
    pub writer: Option<Arc<Mutex<W>>>,
}

impl<R: Input + Send, W: Write + Send> CategoryListIO<R, W> {
    /// Create new CategoryListIO with empty reader and writer
    ///
    /// * `name`: the lists name
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            reader: None,
            writer: None,
        }
    }
}
