use std::{
    fs::{self, File},
    io::Write,
    path::Path,
    sync::Arc,
};

use anyhow::Context;
use bytes::Bytes;
use flume::{Receiver, Sender};
use futures::{future::join_all, lock::Mutex, Future};
use reqwest::Url;
use tokio::task::JoinHandle;

use crate::{
    config::Config,
    input::{url::UrlInput, Input},
};

type SourceDest<SRC, DST> = Vec<(Arc<Mutex<SRC>>, Vec<Arc<Mutex<DST>>>)>;

pub enum ChannelMessage {
    Error(String),
    Info(String),
    Debug(String),
}

pub enum ChannelCommand {
    Quit,
}

/// The FilterController stores the in formation needed to run the data processing
#[derive(Debug)]
pub struct FilterController {
    config: Config,
    message_tx: Sender<ChannelMessage>,
    command_rx: Receiver<ChannelCommand>,
}

impl FilterController {
    pub fn new(
        config: Config,
        command_rx: Receiver<ChannelCommand>,
        message_tx: Sender<ChannelMessage>,
    ) -> Self {
        Self {
            config,
            command_rx,
            message_tx,
        }
    }

    /// Runs the data processing function with UrlInput as input source and a
    /// file as output destination
    pub async fn run(&self) -> anyhow::Result<()> {
        let mut src_dest: SourceDest<UrlInput, File> = vec![];
        fs::create_dir_all(self.config.tmp_dir.clone())
            .with_context(|| format!("could not create temp directory: {}", self.config.tmp_dir))?;
        for (idx, list) in self.config.lists.iter().enumerate() {
            self.message_tx.send(ChannelMessage::Info(format!(
                "processing list: {}",
                list.source
            )))?;
            // create out files
            let mut out_files: Vec<Arc<Mutex<File>>> = vec![];
            let _ = list.tags.iter().map(|tag| -> anyhow::Result<()> {
                let tag_dir = format!("{}/{}", &self.config.tmp_dir, tag);
                fs::create_dir(&tag_dir)
                    .with_context(|| format!("could not create tag directory: {}", tag_dir))?;
                let path_str = format!("{}/{}", &tag_dir, idx);
                let path = Path::new(&path_str);
                let out_file = File::create(path)?;
                out_files.push(Arc::new(Mutex::new(out_file)));
                Ok(())
            });
            // create input source
            let url = Url::parse(&list.source)?;
            let input = UrlInput::new(url, None);
            src_dest.push((Arc::new(Mutex::new(input)), out_files));
        }
        // start processing
        let handles = process(
            src_dest,
            Arc::new(|chunk| async { Ok(chunk) }),
            self.command_rx.clone(),
            self.message_tx.clone(),
        )
        .await;
        join_all(handles).await;
        Ok(())
    }
}

/// `process` is the main data processing function. It reads chunks from the source
/// applies a transformation function and writes the data to the output
async fn process<SRC, DST, FN, RES>(
    source_destination: SourceDest<SRC, DST>,
    fn_transform: Arc<FN>,
    command_rx: Receiver<ChannelCommand>,
    message_tx: Sender<ChannelMessage>,
) -> Vec<JoinHandle<()>>
where
    SRC: Input + Send + Sync + 'static,
    FN: Fn(Bytes) -> RES + Send + Sync + 'static,
    DST: Write + Send + Sync + 'static,
    RES: Future<Output = anyhow::Result<Bytes>> + Send + Sync + 'static,
{
    let mut handles: Vec<JoinHandle<()>> = Vec::new();
    for (input, outputs) in source_destination {
        let reader = Arc::clone(&input);
        let writers: Vec<Arc<Mutex<DST>>> = outputs.iter().map(Arc::clone).collect();
        let fn_trans = Arc::clone(&fn_transform);
        let cmd_rx = command_rx.clone();
        let msg_tx = message_tx.clone();
        let handle = tokio::spawn(async move {
            loop {
                // stop task on quit message
                if let Ok(cmd) = cmd_rx.try_recv() {
                    match cmd {
                        ChannelCommand::Quit => {
                            msg_tx
                                .send(ChannelMessage::Debug("quitting task".to_string()))
                                .unwrap();
                            break;
                        }
                    }
                }

                let result = reader.lock().await.chunk().await;
                match result {
                    Ok(Some(chunk)) => {
                        let chunk = fn_trans(chunk).await.unwrap();
                        for writer in writers.iter() {
                            if let Err(e) = writer.lock().await.write_all(&chunk[..]) {
                                msg_tx
                                    .send(ChannelMessage::Error(format!("{}", e)))
                                    .with_context(|| "error sending ChannelMessage")
                                    .unwrap();
                            }
                        }
                    }
                    Ok(None) => {
                        break;
                    }
                    Err(e) => {
                        msg_tx
                            .send(ChannelMessage::Error(format!("{}", e)))
                            .with_context(|| "error sending ChannelMessage")
                            .unwrap();
                        break;
                    }
                }
            }
        });
        handles.push(handle);
    }
    handles
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use bytes::Bytes;
    use std::io::{Cursor, Read};

    use super::*;

    #[derive(Debug)]
    struct TestInput {
        cursor: Cursor<String>,
    }

    #[async_trait]
    impl Input for TestInput {
        async fn chunk(&mut self) -> anyhow::Result<Option<Bytes>> {
            let mut buf = vec![0; 32];
            let n = self.cursor.read(&mut buf)?;
            if n == 0 {
                Ok(None)
            } else {
                Ok(Some(Bytes::from(buf)))
            }
        }
    }

    #[tokio::test]
    async fn test_process() {
        let input_data = "line one\nline two\n".to_string();
        let cursor = Cursor::new(input_data.clone());
        let input = Arc::new(Mutex::new(TestInput { cursor }));
        let output = Arc::new(Mutex::new(Cursor::new(vec![0, 32])));
        let outputs = vec![Arc::clone(&output)];
        let (_, cmd_rx): (Sender<ChannelCommand>, Receiver<ChannelCommand>) = flume::unbounded();
        let (msg_tx, _): (Sender<ChannelMessage>, Receiver<ChannelMessage>) = flume::unbounded();
        let handles = process(
            vec![(Arc::clone(&input), outputs)],
            Arc::new(|c| async { Ok(c) }),
            cmd_rx,
            msg_tx,
        )
        .await;
        join_all(handles).await;
        let o = output.lock().await.clone().into_inner();
        assert!(String::from_utf8_lossy(&o).starts_with(&input_data));
    }
}
