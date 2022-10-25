use std::{io::Write, sync::Arc};

use anyhow::Context;
use futures::lock::Mutex;
use std::fs::File;

use crate::{filter_list::FilterList, input::Input};

/// FilterListIO is a wrapper type for FilterList objects which additionally
/// contains input sources and output writers. The wrapper is necessary to
/// keep the FilterList itself serializable.
#[derive(Debug)]
pub struct FilterListIO<R: Input + Send, W: Write + Send> {
    pub filter_list: FilterList,
    pub reader: Option<Arc<Mutex<R>>>,
    pub writer: Option<Arc<Mutex<W>>>,
}

impl<R: Input + Send, W: Write + Send> FilterListIO<R, W> {
    pub fn new(filter_list: FilterList) -> Self {
        Self {
            filter_list,
            reader: None,
            writer: None,
        }
    }

    /// returns the reader's content length
    pub async fn reader_len(&mut self) -> anyhow::Result<u64> {
        if self.reader.is_none() {
            return Err(anyhow::anyhow!("reader attribute is None"));
        }
        let mut reader = self.reader.as_mut().unwrap().lock().await;
        let length = reader.len().await?;
        Ok(length)
    }
}

impl<R: Input + Send> FilterListIO<R, File> {
    /// returns the writer's content length
    pub async fn writer_len(&self) -> anyhow::Result<u64> {
        if self.writer.is_none() {
            return Err(anyhow::anyhow!("writer attribute is None"));
        }
        let file = self.writer.as_ref().unwrap().lock().await;
        let file_meta = file
            .metadata()
            .with_context(|| format!("file {:?} has no metadata", file))?;
        let file_len = file_meta.len();
        Ok(file_len)
    }

    /// is_cached compares the reader's length to the writer's length
    /// if both are equal we assume no further action will be necessary
    pub async fn is_cached(&mut self) -> anyhow::Result<bool> {
        let r_len = match self.reader_len().await {
            Ok(l) => l,
            Err(e) => {
                warn!("{}", e);
                return Ok(false);
            }
        };
        let w_len = match self.writer_len().await {
            Ok(l) => l,
            Err(e) => {
                debug!("{}", e);
                return Ok(false);
            }
        };
        debug!(
            "List {} has reader length: {}, writer length: {}",
            self.filter_list.id, r_len, w_len
        );
        Ok(r_len == w_len)
    }
}
