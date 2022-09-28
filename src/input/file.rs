use std::{
    fs::File,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
};

use crate::input::Input;
use anyhow::Context;
use async_trait::async_trait;
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use tar::Archive;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", content = "archive_list_file")]
pub enum Compression {
    Gz,
    TarGz(String),
}

pub enum Handle {
    File(BufReader<File>),
    Gz(GzDecoder<File>),
    TarGz(Archive<GzDecoder<File>>),
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
            Some(Compression::Gz) => {
                let gz = GzDecoder::new(f);
                self.handle = Some(Handle::Gz(gz));
            }
            Some(Compression::TarGz(_file_name)) => {
                let gz = GzDecoder::new(f);
                let archive = Archive::new(gz);
                self.handle = Some(Handle::TarGz(archive));
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
            Handle::File(file) => match file.read_line(&mut str_buf) {
                Ok(n) if n > 0 => Ok(Some(str_buf.as_bytes().to_vec())),
                Ok(n) if n == 0 => Ok(None),
                Ok(_) => Ok(None),
                Err(e) => Err(anyhow::anyhow!("Error reading line from file: {}", e)),
            },
            Handle::Gz(archive) => match archive.read(&mut buf) {
                Ok(n) if n > 0 => Ok(Some(Vec::from(buf))),
                Ok(n) if n == 0 => Ok(None),
                Ok(_) => Ok(None),
                Err(e) => Err(anyhow::anyhow!("Error reading chunk from file: {}", e)),
            },
            Handle::TarGz(archive) => {
                if let Some(compression) = &self.compression {
                    println!("call");
                    match compression {
                        Compression::TarGz(wanted_path_str) => {
                            let path_wanted = Path::new(wanted_path_str);
                            let entry = archive.entries()?.find(|entry| {
                                if let Ok(entry) = entry
                                    && let Ok(path) = entry.path()
                                    && path == path_wanted
                                {
                                    return true;
                                }
                                false
                            });

                            if entry.is_none() {
                                return Err(anyhow::anyhow!(
                                    "specified list file not found in archive"
                                ));
                            }

                            match entry.unwrap().unwrap().read(&mut buf) {
                                Ok(n) if n > 0 => return Ok(Some(Vec::from(buf))),
                                Ok(n) if n == 0 => return Ok(None),
                                Ok(_) => return Ok(None),
                                Err(e) => {
                                    return Err(anyhow::anyhow!(
                                        "Error reading chunk from file: {}",
                                        e
                                    ))
                                }
                            }
                        }
                        _ => return Err(anyhow::anyhow!("compression must be TarGz")),
                    }
                }
                Err(anyhow::anyhow!("compression must be TarGz"))
            }
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
