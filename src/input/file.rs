use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::PathBuf,
};

use crate::input::Input;
use async_trait::async_trait;
use bytes::Bytes;

/// UrlInput downloads data from an Url
#[derive(Debug)]
pub(crate) struct FileInput {
    path: PathBuf,
    handle: Option<BufReader<File>>,
}

impl FileInput {
    pub fn new(path: PathBuf) -> Self {
        Self { path, handle: None }
    }
}

#[async_trait]
impl Input for FileInput {
    async fn chunk(&mut self) -> anyhow::Result<Option<Bytes>> {
        if self.handle.is_none() {
            let f = File::open(self.path.clone())?;
            self.handle = Some(BufReader::new(f));
        }
        let mut buf = String::new();
        match self.handle.as_mut().unwrap().read_line(&mut buf) {
            Ok(n) if n > 0 => {
                let b = Bytes::from(buf);
                Ok(Some(b))
            }
            Ok(n) if n == 0 => Ok(None),
            Ok(_) => Ok(None),
            Err(e) => Err(anyhow::anyhow!(e)),
        }
    }
}
