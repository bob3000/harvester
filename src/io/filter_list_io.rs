use std::{fs, io::Write, path::Path, sync::Arc};

use anyhow::Context;
use futures::lock::Mutex;
use reqwest::Url;
use std::fs::File;

use crate::{
    filter_list::FilterList,
    input::{
        file::{Compression, FileInput},
        url::UrlInput,
        Input,
    },
};

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

impl<W: Write + Send> FilterListIO<UrlInput, W> {
    /// configures input to read from HTTP response
    pub fn attach_url_reader(&mut self) -> anyhow::Result<()> {
        let url = Url::parse(&self.filter_list.source)
            .with_context(|| format!("config file error: {:?}", &self.filter_list))?;
        let input = UrlInput::new(url);
        self.reader = Some(Arc::new(Mutex::new(input)));
        Ok(())
    }
}

impl<W: Write + Send> FilterListIO<FileInput, W> {
    /// Searches the file system in the given base directory for a file named after the list id. If the
    /// file was found it's being opened for reading and the reader is attached to the FilterListIO or
    /// otherwise returns an error.
    ///
    /// * `base_dir`: the file system path to be searched
    /// * `compression`: file compression method to be expected
    pub fn attach_existing_input_file(
        &mut self,
        base_dir: &Path,
        compression: Option<Compression>,
    ) -> anyhow::Result<()> {
        let mut contents =
            fs::read_dir(base_dir).with_context(|| "input file directory does not exist")?;
        let entry = contents
            .find(|it| {
                if let Ok(it) = it {
                    return it.file_name().to_str().unwrap() == self.filter_list.id;
                }
                false
            })
            .ok_or_else(|| anyhow::anyhow!("file not found: {}", self.filter_list.id))??;
        let path = entry.path();
        let file_name = path.as_os_str().to_str().unwrap();
        match entry.metadata() {
            Ok(meta) => {
                if meta.len() == 0 {
                    debug!("File {} has zero length", file_name);
                    return Ok(());
                };
            }
            Err(_) => {
                debug!("File {} has no length", file_name);
                return Ok(());
            }
        };
        self.reader = Some(Arc::new(Mutex::new(FileInput::new(
            entry.path(),
            compression,
        ))));
        Ok(())
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
            .with_context(|| format!("file {file:?} has no metadata"))?;
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

    /// Tries to read the potential output file for inspection
    ///
    /// * `base_dir`: the base directory where the output file is tried to read
    pub fn attach_existing_file_writer(&mut self, base_dir: &Path) -> anyhow::Result<()> {
        let out_path = base_dir;
        let mut out_path = out_path.to_path_buf();
        out_path.push(&self.filter_list.id);
        let out_file =
            File::open(out_path).with_context(|| "could not open out file for reading")?;
        self.writer = Some(Arc::new(Mutex::new(out_file)));
        Ok(())
    }

    /// Creates and output file and it's parent directories, opens the file for writing
    /// and attaches it to the given FilterListIO object
    ///
    /// * `base_dir`: the base directory where the output file is being created
    pub fn attach_new_file_writer(&mut self, base_dir: &Path) -> anyhow::Result<()> {
        let mut out_path = base_dir.to_path_buf();
        fs::create_dir_all(&out_path).with_context(|| "could not create out directory")?;
        out_path.push(&self.filter_list.id);
        let out_file = File::create(out_path).with_context(|| "could not write out file")?;
        self.writer = Some(Arc::new(Mutex::new(out_file)));
        Ok(())
    }
}
