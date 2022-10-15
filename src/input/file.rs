use std::path::{Path, PathBuf};

use crate::input::Input;
use anyhow::Context;
use async_compression::tokio::bufread::GzipDecoder;
use async_trait::async_trait;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::{
    fs::File,
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader},
};
use tokio_tar::{Archive, Entry};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", content = "archive_list_file")]
pub enum Compression {
    Gz,
    TarGz(String),
}

pub enum Handle {
    File(BufReader<File>),
    Gz(GzipDecoder<BufReader<File>>),
    TarGz(Entry<Archive<GzipDecoder<BufReader<File>>>>),
}

/// FileInput reads data from a File
pub struct FileInput {
    /// file compression method used
    compression: Option<Compression>,
    /// path on the file system
    path: PathBuf,
    /// the file handle
    handle: Option<Handle>,
}

impl FileInput {
    /// Crates new file input
    ///
    /// * `path`: path on the file system
    /// * `compression`: the files compression to be expected
    pub fn new(path: PathBuf, compression: Option<Compression>) -> Self {
        Self {
            compression,
            path,
            handle: None,
        }
    }

    /// initializes the file handle according to the specified compression format
    async fn init_handle(&mut self) -> anyhow::Result<()> {
        let f = File::open(self.path.clone()).await.with_context(|| {
            format!(
                "unable to open file {}",
                self.path
                    .file_name()
                    .unwrap_or_default()
                    .to_str()
                    .unwrap_or_default()
            )
        })?;
        match &self.compression {
            Some(Compression::Gz) => {
                let gz = GzipDecoder::new(BufReader::new(f));
                self.handle = Some(Handle::Gz(gz));
            }
            Some(Compression::TarGz(wanted_path_str)) => {
                let gz = GzipDecoder::new(BufReader::new(f));
                let mut archive = Archive::new(gz);

                let path_wanted = Path::new(wanted_path_str);
                let mut entries = archive.entries()?;
                while let Some(entry_result) = entries.next().await {
                    if let Ok(entry) = entry_result
                        && let Ok(path) = entry.path()
                        && path == path_wanted
                    {
                        self.handle = Some(Handle::TarGz(entry));
                        break;
                    }
                }
                if self.handle.is_none() {
                    return Err(anyhow::anyhow!("specified list file not found in archive"));
                }
            }
            None => self.handle = Some(Handle::File(BufReader::new(f))),
        }
        Ok(())
    }
}

#[async_trait]
impl Input for FileInput {
    async fn chunk(&mut self) -> anyhow::Result<Option<Vec<u8>>> {
        /// inner function reading bytes until the next newline character
        ///
        /// * `archive`: the file handle to read from
        /// * `vec_buf`: the target buffer containing the line
        async fn read_bytes_to_newline(
            archive: &mut (impl AsyncRead + Unpin),
            mut vec_buf: Vec<u8>,
        ) -> anyhow::Result<Option<Vec<u8>>> {
            loop {
                let mut byte_buf = Vec::with_capacity(1);
                let n = archive.take(1).read_to_end(&mut byte_buf).await;
                match n {
                    Ok(n) if n > 0 => {
                        if let Some(b) = byte_buf.last() && b == &10 {
                                return Ok(Some(vec_buf));
                            }
                        vec_buf.extend(byte_buf);
                        if vec_buf.len() >= vec_buf.capacity() {
                            return Err(anyhow::anyhow!("Error reading chunk from file: line lenght exceedes buffer capacity"));
                        }
                    }
                    Err(e) => return Err(anyhow::anyhow!("Error reading chunk from file: {}", e)),
                    _ => return Ok(None),
                }
            }
        }

        // read buffer size for a single line
        const BUF_SIZE: usize = 1024;

        if self.handle.is_none() {
            self.init_handle().await?;
        }
        let mut str_buf = String::new();
        let vec_buf = Vec::with_capacity(BUF_SIZE);
        // handle can be safely unwrapped here since it's initialized at the beginning of the function
        match self.handle.as_mut().unwrap() {
            Handle::File(file) => match file.read_line(&mut str_buf).await {
                Ok(n) if n > 0 => Ok(Some(str_buf.as_bytes().to_vec())),
                Ok(n) if n == 0 => Ok(None),
                Ok(_) => Ok(None),
                Err(e) => Err(anyhow::anyhow!("Error reading line from file: {}", e)),
            },
            Handle::Gz(archive) => read_bytes_to_newline(archive, vec_buf).await,
            Handle::TarGz(archive) => read_bytes_to_newline(archive, vec_buf).await,
        }
    }

    /// reinitialize the file handle and start reading from zero
    async fn reset(&mut self) -> anyhow::Result<()> {
        if self.handle.is_some() {
            self.handle.take();
        }
        self.init_handle().await?;
        Ok(())
    }
}
