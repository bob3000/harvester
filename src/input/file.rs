use std::{
    fs::File,
    io::{BufRead, BufReader, Read},
    path::PathBuf,
};

use crate::input::Input;
use anyhow::Context;
use async_trait::async_trait;
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum Compression {
    Gz,
}

#[derive(Debug)]
pub enum Handle {
    File(BufReader<File>),
    Gz(GzDecoder<File>),
}

impl Read for Handle {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::File(f) => f.read(buf),
            Self::Gz(f) => f.read(buf),
        }
    }
}

/// FileInput reads data from a File
#[derive(Debug)]
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

    fn init_handle(&mut self) -> anyhow::Result<()> {
        let f = File::open(self.path.clone()).with_context(|| {
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
            Some(_) => {
                let gz = GzDecoder::new(f);
                self.handle = Some(Handle::Gz(gz));
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
            self.init_handle()?;
        }
        let mut buf = [0; 1024];
        let mut str_buf = String::new();
        match self.handle.as_mut().unwrap() {
            Handle::Gz(archive) => match archive.read(&mut buf) {
                Ok(n) if n > 0 => Ok(Some(Vec::from(buf))),
                Ok(n) if n == 0 => Ok(None),
                Ok(_) => Ok(None),
                Err(e) => Err(anyhow::anyhow!("Error reading chunk from file: {}", e)),
            },
            Handle::File(file) => match file.read_line(&mut str_buf) {
                Ok(n) if n > 0 => Ok(Some(str_buf.as_bytes().to_vec())),
                Ok(n) if n == 0 => Ok(None),
                Ok(_) => Ok(None),
                Err(e) => Err(anyhow::anyhow!("Error reading line from file: {}", e)),
            },
        }
    }

    async fn reset(&mut self) -> anyhow::Result<()> {
        if self.handle.is_some() {
            self.handle.take();
        }
        self.init_handle()?;
        Ok(())
    }
}
