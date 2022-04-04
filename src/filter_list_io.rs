use std::{io::Write, sync::Arc};

use futures::lock::Mutex;

use crate::{filter_list::FilterList, input::Input};

/// FilterListIO is a wrapper type for FilterList objects which additionally
/// contains input sources and output writers. The wrapper is necessary to
/// keep the FilterList itself serializable.
#[derive(Debug)]
pub struct FilterListIO<R: Input, W: Write> {
    pub filter_list: FilterList,
    pub reader: Option<Arc<Mutex<R>>>,
    pub writer: Option<Arc<Mutex<W>>>,
}

impl<R: Input, W: Write> FilterListIO<R, W> {
    pub fn new(filter_list: FilterList) -> Self {
        Self {
            filter_list,
            reader: None,
            writer: None,
        }
    }
}
