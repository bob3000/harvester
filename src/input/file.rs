use std::path::{Path, PathBuf};

use crate::input::Input;
use anyhow::Context;
use async_compression::tokio::bufread::GzipDecoder;
use async_trait::async_trait;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::{
    fs::File,
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
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

// impl Read for Handle {
//     fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
//         match self {
//             Self::File(f) => f.read(buf),
//             Self::Gz(f) => f.read(buf),
//             Self::TarGz(f) => f.read(buf),
//         }
//     }
// }

/// FileInput reads data from a File
pub struct FileInput {
    compression: Option<Compression>,
    path: PathBuf,
    handle: Option<Handle>,
}

impl FileInput {
    pub fn new(path: PathBuf, compression: Option<Compression>) -> Self {
        Self {
            compression,
            path,
            handle: None,
        }
    }

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
                        println!("found path: {:?}", path);
                        self.handle = Some(Handle::TarGz(entry));
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
        if self.handle.is_none() {
            self.init_handle().await?;
        }
        let mut buf = [0; 1024];
        let mut str_buf = String::new();
        match self.handle.as_mut().unwrap() {
            Handle::File(file) => match file.read_line(&mut str_buf).await {
                Ok(n) if n > 0 => Ok(Some(str_buf.as_bytes().to_vec())),
                Ok(n) if n == 0 => Ok(None),
                Ok(_) => Ok(None),
                Err(e) => Err(anyhow::anyhow!("Error reading line from file: {}", e)),
            },
            Handle::Gz(archive) => match archive.read(&mut buf[..]).await {
                Ok(n) if n > 0 => {
                    println!("reading {:?}", buf);
                    Ok(Some(Vec::from(&buf[..n])))
                }
                Ok(n) if n == 0 => Ok(None),
                Ok(_) => Ok(None),
                Err(e) => Err(anyhow::anyhow!("Error reading chunk from file: {}", e)),
            },
            Handle::TarGz(archive_entry) => {
                // println!("entry {:?}", archive_entry);
                match archive_entry.read(&mut buf[..]).await {
                    Ok(n) if n > 0 => {
                        println!("reading {}, {:?}", n, buf);
                        Ok(Some(Vec::from(&buf[..])))
                    }
                    Ok(n) if n == 0 => Ok(None),
                    Ok(_) => Ok(None),
                    Err(e) => Err(anyhow::anyhow!("Error reading chunk from file: {}", e)),
                }
            }
        }
    }

    async fn reset(&mut self) -> anyhow::Result<()> {
        if self.handle.is_some() {
            self.handle.take();
        }
        self.init_handle().await?;
        Ok(())
    }
}
