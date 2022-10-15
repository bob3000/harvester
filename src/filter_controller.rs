use std::{
    fs::{self, File},
    io::Write,
    marker::PhantomData,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use anyhow::Context;
use flume::Sender;
use futures::{lock::Mutex, Future};
use reqwest::Url;
use tokio::task::JoinHandle;

use crate::{
    config::Config,
    filter_list::FilterList,
    input::{
        file::{Compression, FileInput},
        url::UrlInput,
        Input,
    },
    io::category_list_io::CategoryListIO,
    io::filter_list_io::FilterListIO,
};

/// Sub path for downloaded raw lists
pub const RAW_PATH: &str = "raw";
/// Sub path for transformed lists
pub const TRANSFORM_PATH: &str = "transform";
/// Sub path for the assembled categorized lists
pub const CATEGORIZE_PATH: &str = "categorize";

// enum specifying the severity level of a channel message
pub enum ChannelMessage {
    Error(String),
    Info(String),
    Debug(String),
    Shutdown,
}

/// These structs represent the stages of a program run
pub struct StageDownload;
pub struct StageExtract;
pub struct StageCategorize;
pub struct StageOutput;

/// The FilterController stores the in formation needed to run the data processing
#[derive(Debug)]
pub struct FilterController<Stage, R: Input + Send, W: Write + Send> {
    pub stage: PhantomData<Stage>,
    pub config: Config,
    pub message_tx: Sender<ChannelMessage>,
    pub filter_lists: Vec<FilterListIO<R, W>>,
    pub category_lists: Vec<CategoryListIO<R, W>>,
    pub is_processing: Arc<AtomicBool>,
}

/// Searches the file system in the given base directory for a file named after the list id. If the
/// file was found it's being opened for reading and the reader is attached to the FilterListIO or
/// otherwise returns an error.
///
/// * `list`: the FilterListIO where the reader
/// * `base_dir`: the file system path to be searched
pub fn get_input_file<W: Write + Send>(
    list: &mut FilterListIO<FileInput, W>,
    base_dir: PathBuf,
    compression: Option<Compression>,
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
    list.reader = Some(Arc::new(Mutex::new(FileInput::new(
        entry.path(),
        compression,
    ))));
    Ok(())
}

/// Turns the source string from the configuration into an Url object and attaches
/// it to the FilterListIO object
///
/// * `list`: the FilterListIO object to receive the URL object
pub fn create_input_urls(list: &mut FilterListIO<UrlInput, File>) -> anyhow::Result<()> {
    let url = Url::parse(&list.filter_list.source)?;
    let input = UrlInput::new(url);
    list.reader = Some(Arc::new(Mutex::new(input)));
    Ok(())
}

/// Creates and output file and it's parent directories, opens the file for writing
/// and attaches it to the given FilterListIO object
///
/// * `list`: the FilterListIO object to receive the writer
/// * `base_dir`: the base directory where the output directory is being created
pub fn create_out_file<R: Input + Send>(
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
///
/// * `filter_lists`: a list of FilterListIO to be processed
/// * `fn_transform`: the function to apply to every chunk the FilterListIO's reader returns
/// * `command_rx`: a channel receiver listening for commands
/// * `message_tx`: a channel sender for messaging purpose
pub async fn process<SRC, DST, FN, RES>(
    filter_lists: &mut Vec<FilterListIO<SRC, DST>>,
    fn_transform: &'static FN,
    message_tx: Sender<ChannelMessage>,
    is_processing: Arc<AtomicBool>,
) -> Vec<JoinHandle<()>>
where
    SRC: Input + Send + 'static,
    FN: Fn(Arc<FilterList>, Option<Vec<u8>>) -> RES + Send + Sync + 'static,
    DST: Write + Send + 'static,
    RES: Future<Output = anyhow::Result<Option<Vec<u8>>>> + Send + Sync + 'static,
{
    let mut handles: Vec<JoinHandle<()>> = Vec::new();
    for FilterListIO {
        reader,
        writer,
        filter_list,
        ..
    } in filter_lists
    {
        if !is_processing.load(Ordering::SeqCst) {
            return handles;
        }
        let reader = match &reader.take() {
            Some(r) => Arc::clone(r),
            None => {
                debug!("reader is None: {}", filter_list.id);
                continue;
            }
        };
        let writer = match &writer.take() {
            Some(w) => Arc::clone(w),
            None => {
                debug!("writer is None: {}", filter_list.id);
                continue;
            }
        };
        let filter_list = Arc::new(filter_list.clone());
        let list = Arc::clone(&filter_list);
        let msg_tx = message_tx.clone();

        msg_tx
            .send(ChannelMessage::Info(format!(
                "{}: {}",
                filter_list.id, filter_list.source
            )))
            .unwrap_or_else(|m| {
                debug!("filter_controller: {}", m);
            });
        let is_proc = Arc::clone(&is_processing);
        let handle = tokio::spawn(async move {
            loop {
                if !is_proc.load(Ordering::SeqCst) {
                    msg_tx
                        .send(ChannelMessage::Debug("quitting task".to_string()))
                        .unwrap_or_else(|m| {
                            debug!("filter_controller: {}", m);
                        });
                    return;
                }
                // stop task on quit message
                let result = reader.lock().await.chunk().await;
                match result {
                    Ok(Some(chunk)) => {
                        let chunk = fn_transform(list.clone(), Some(chunk)).await.unwrap();
                        if let Some(chunk) = chunk {
                            if let Err(e) = writer.lock().await.write_all(&chunk) {
                                msg_tx
                                    .send(ChannelMessage::Error(format!("{}", e)))
                                    .with_context(|| "error sending ChannelMessage")
                                    .unwrap_or_else(|m| {
                                        debug!("filter_controller: {}", m);
                                    });
                            }
                        }
                    }
                    Ok(None) => {
                        break;
                    }
                    Err(e) => {
                        msg_tx
                            .send(ChannelMessage::Error(format!("Error: {}", e)))
                            .with_context(|| "error sending ChannelMessage")
                            .unwrap_or_else(|m| {
                                debug!("filter_controller: {}", m);
                            });
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
    use flume::Receiver;
    use futures::future::join_all;
    use std::io::{Cursor, Read};

    use super::*;

    /// TestInput implements the Input trait using Cursors
    #[derive(Debug)]
    struct TestInput {
        cursor: Cursor<String>,
    }

    #[async_trait]
    impl Input for TestInput {
        async fn chunk(&mut self) -> anyhow::Result<Option<Vec<u8>>> {
            let mut buf = vec![0; 32];
            let n = self.cursor.read(&mut buf)?;
            if n == 0 {
                Ok(None)
            } else {
                Ok(Some(buf.to_vec()))
            }
        }

        async fn reset(&mut self) -> anyhow::Result<()> {
            self.cursor.set_position(0);
            Ok(())
        }
    }

    #[tokio::test]
    /// tests the `process` function using the TestInput to avoid writing files
    async fn test_process() {
        // create input data
        let input_data = "line one\nline two\n".to_string();
        let cursor = Cursor::new(input_data.clone());
        let input = Arc::new(Mutex::new(TestInput { cursor }));
        // set up output sink
        let output = Arc::new(Mutex::new(Cursor::new(vec![0, 32])));
        let (msg_tx, _): (Sender<ChannelMessage>, Receiver<ChannelMessage>) = flume::unbounded();

        let is_processing = Arc::new(AtomicBool::new(true));

        // apply the data to the FilterList object
        let filter_list = FilterList {
            id: "".to_string(),
            compression: None,
            source: "".to_string(),
            tags: vec![],
            regex: "".to_string(),
        };

        // wrap the Filterlist in the FilterListIO object
        let mut filter_list_io: FilterListIO<TestInput, Cursor<Vec<u8>>> =
            FilterListIO::new(filter_list);
        filter_list_io.reader = Some(input);
        filter_list_io.writer = Some(output.clone());

        // process the data with a transform function just forwarding the data
        let handles = process(
            &mut vec![filter_list_io],
            &|_, c| async { Ok(c) },
            msg_tx,
            is_processing.clone(),
        )
        .await;
        join_all(handles).await;
        let o = output.lock().await.clone().into_inner();

        // the data in the out put should be the same as the input data
        assert!(String::from_utf8_lossy(&o).starts_with(&input_data));
    }
}
