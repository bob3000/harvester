use std::{
    fs::{self, File},
    io::Write,
    marker::PhantomData,
    path::PathBuf,
    sync::Arc,
};

use anyhow::Context;
use flume::{Receiver, Sender};
use futures::{lock::Mutex, Future};
use reqwest::Url;
use tokio::task::JoinHandle;

use crate::{
    config::Config,
    filter_list::FilterList,
    input::{file::FileInput, url::UrlInput, Input},
    io::category_list_io::CategoryListIO,
    io::filter_list_io::FilterListIO,
};

/// Sub path for downloaded raw lists
pub const RAW_PATH: &str = "raw";
/// Sub path for transformed lists
pub const TRANSFORM_PATH: &str = "transform";
/// Sub path for the assembled categorized lists
pub const CATEGORIZE_PATH: &str = "categorize";

pub enum ChannelMessage {
    Error(String),
    Info(String),
    Debug(String),
}

pub enum ChannelCommand {
    Quit,
}

/// These structs represent the stages of a program run
pub struct StageDownload;
pub struct StageExtract;
pub struct StageCategorize;
pub struct StageOutput;

/// The FilterController stores the in formation needed to run the data processing
#[derive(Debug)]
pub struct FilterController<Stage, R: Input + Send + Sync, W: Write + Send + Sync> {
    pub stage: PhantomData<Stage>,
    pub config: Config,
    pub message_tx: Sender<ChannelMessage>,
    pub command_rx: Receiver<ChannelCommand>,
    pub filter_lists: Vec<FilterListIO<R, W>>,
    pub category_lists: Vec<CategoryListIO<R, W>>,
}

pub fn get_input_file<W: Write + Send + Sync>(
    list: &mut FilterListIO<FileInput, W>,
    base_dir: PathBuf,
) -> anyhow::Result<()> {
    let mut contents =
        fs::read_dir(base_dir).with_context(|| "input file directory does not exist")?;
    let entry = contents
        .find(|it| {
            if let Ok(it) = it {
                return it.file_name().to_str().unwrap() == list.filter_list.id;
            }
            false
        })
        .ok_or_else(|| anyhow::anyhow!("file not found: {}", list.filter_list.id))??;
    list.reader = Some(Arc::new(Mutex::new(FileInput::new(entry.path()))));
    Ok(())
}

pub fn create_input_urls(list: &mut FilterListIO<UrlInput, File>) -> anyhow::Result<()> {
    let url = Url::parse(&list.filter_list.source)?;
    let input = UrlInput::new(url);
    list.reader = Some(Arc::new(Mutex::new(input)));
    Ok(())
}

pub fn create_out_file<R: Input + Send + Sync>(
    list: &mut FilterListIO<R, File>,
    base_dir: PathBuf,
) -> anyhow::Result<()> {
    let mut out_path = base_dir;
    fs::create_dir_all(&out_path).with_context(|| "could not create out directory")?;
    out_path.push(&list.filter_list.id);
    let out_file = File::create(out_path).with_context(|| "could not write out file")?;
    list.writer = Some(Arc::new(Mutex::new(out_file)));
    Ok(())
}

/// `process` is the main data processing function. It reads chunks from the source
/// applies a transformation function and writes the data to the output
pub async fn process<SRC, DST, FN, RES>(
    filter_lists: &mut Vec<FilterListIO<SRC, DST>>,
    fn_transform: &'static FN,
    command_rx: Receiver<ChannelCommand>,
    message_tx: Sender<ChannelMessage>,
) -> Vec<JoinHandle<()>>
where
    SRC: Input + Send + Sync + 'static,
    FN: Fn(Arc<FilterList>, Option<String>) -> RES + Send + Sync + 'static,
    DST: Write + Send + Sync + 'static,
    RES: Future<Output = anyhow::Result<Option<String>>> + Send + Sync + 'static,
{
    let mut handles: Vec<JoinHandle<()>> = Vec::new();
    for FilterListIO {
        reader,
        writer,
        filter_list,
        ..
    } in filter_lists
    {
        let reader = Arc::clone(&reader.take().unwrap());
        let writer = Arc::clone(&writer.take().unwrap());
        let filter_list = Arc::new(filter_list.clone());
        let list = Arc::clone(&filter_list);
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
                        let chunk = fn_transform(list.clone(), Some(chunk)).await.unwrap();
                        if let Some(chunk) = chunk {
                            if let Err(e) = writer.lock().await.write_all(chunk.as_bytes()) {
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
    use crate::filter_list::FilterList;
    use async_trait::async_trait;
    use futures::future::join_all;
    use std::io::{Cursor, Read};

    use super::*;

    #[derive(Debug)]
    struct TestInput {
        cursor: Cursor<String>,
    }

    #[async_trait]
    impl Input for TestInput {
        async fn chunk(&mut self) -> anyhow::Result<Option<String>> {
            let mut buf = vec![0; 32];
            let n = self.cursor.read(&mut buf)?;
            if n == 0 {
                Ok(None)
            } else {
                Ok(Some(String::from_utf8(buf.to_vec()).unwrap()))
            }
        }

        async fn reset(&mut self) -> anyhow::Result<()> {
            self.cursor.set_position(0);
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_process() {
        let input_data = "line one\nline two\n".to_string();
        let cursor = Cursor::new(input_data.clone());
        let input = Arc::new(Mutex::new(TestInput { cursor }));
        let output = Arc::new(Mutex::new(Cursor::new(vec![0, 32])));
        let (_, cmd_rx): (Sender<ChannelCommand>, Receiver<ChannelCommand>) = flume::unbounded();
        let (msg_tx, _): (Sender<ChannelMessage>, Receiver<ChannelMessage>) = flume::unbounded();
        let filter_list = FilterList {
            id: "".to_string(),
            source: "".to_string(),
            tags: vec![],
            regex: "".to_string(),
        };
        let mut filter_list_io: FilterListIO<TestInput, Cursor<Vec<u8>>> =
            FilterListIO::new(filter_list);
        filter_list_io.reader = Some(input);
        filter_list_io.writer = Some(output.clone());
        let handles = process(
            &mut vec![filter_list_io],
            &|_, c| async { Ok(c) },
            cmd_rx,
            msg_tx,
        )
        .await;
        join_all(handles).await;
        let o = output.lock().await.clone().into_inner();
        assert!(String::from_utf8_lossy(&o).starts_with(&input_data));
    }
}
