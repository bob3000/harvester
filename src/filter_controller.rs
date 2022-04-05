use std::{
    fs::{self, File},
    io::Write,
    path::PathBuf,
    str::FromStr,
    sync::Arc,
};

use anyhow::Context;
use flume::{Receiver, Sender};
use futures::{future::join_all, lock::Mutex, Future};
use regex::Regex;
use reqwest::Url;
use tokio::task::JoinHandle;

use crate::{
    config::Config,
    filter_list::FilterList,
    filter_list_io::FilterListIO,
    input::{file::FileInput, url::UrlInput, Input},
};

/// Sub path for downloaded raw lists
const RAW_PATH: &str = "raw";
/// Sub path for transformed lists
const TRANSFORM_PATH: &str = "transform";

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
pub struct FilterController<R: Input, W: Write> {
    config: Config,
    message_tx: Sender<ChannelMessage>,
    command_rx: Receiver<ChannelCommand>,
    filter_lists: Vec<FilterListIO<R, W>>,
}

/// This implementation for UrlInput and File is the first phase where the lists
/// are downloaded.
impl FilterController<UrlInput, File> {
    pub fn new(
        config: Config,
        command_rx: Receiver<ChannelCommand>,
        message_tx: Sender<ChannelMessage>,
    ) -> Self {
        Self {
            config,
            command_rx,
            message_tx,
            filter_lists: vec![],
        }
    }

    /// Runs the data processing function with UrlInput as input source and a
    /// file as output destination
    pub async fn run(&mut self) -> anyhow::Result<FilterController<FileInput, File>> {
        let mut raw_path = PathBuf::from_str(&self.config.tmp_dir)?;
        raw_path.push(RAW_PATH);

        self.prepare_download(raw_path.clone())?;
        self.download().await?;
        let transform_controller = FilterController::<FileInput, File> {
            config: self.config.clone(),
            command_rx: self.command_rx.clone(),
            message_tx: self.message_tx.clone(),
            filter_lists: vec![],
        };
        Ok(transform_controller)
    }

    /// Equips the FilterListIO objects with a reader and writers
    fn prepare_download(&mut self, raw_path: PathBuf) -> anyhow::Result<()> {
        self.filter_lists = self
            .config
            .lists
            .iter()
            .map(|f| FilterListIO::new(f.clone()))
            .collect();
        self.filter_lists
            .iter_mut()
            .try_for_each(|l| -> anyhow::Result<()> {
                create_input_urls(l)?;
                create_out_file(l, raw_path.clone())?;
                Ok(())
            })?;
        Ok(())
    }

    /// downloads lists to temp files
    async fn download(&mut self) -> anyhow::Result<()> {
        let handles = process(
            &mut self.filter_lists,
            &|_, chunk| async { Ok(chunk) },
            self.command_rx.clone(),
            self.message_tx.clone(),
        )
        .await;
        join_all(handles).await;
        Ok(())
    }
}

/// This implementation for FileInput and File is the second stage where URLs are
/// being extracted
impl FilterController<FileInput, File> {
    pub async fn run(&mut self) -> anyhow::Result<()> {
        let mut raw_path = PathBuf::from_str(&self.config.tmp_dir)?;
        raw_path.push(RAW_PATH);
        let mut trans_path = PathBuf::from_str(&self.config.tmp_dir)?;
        trans_path.push(TRANSFORM_PATH);

        self.prepare_extract(raw_path.clone(), trans_path.clone())?;
        self.extract().await?;
        Ok(())
    }

    fn prepare_extract(&mut self, raw_path: PathBuf, trans_path: PathBuf) -> anyhow::Result<()> {
        self.filter_lists = self
            .config
            .lists
            .iter()
            .map(|f| FilterListIO::new(f.clone()))
            .collect();
        self.filter_lists
            .iter_mut()
            .try_for_each(|l| -> anyhow::Result<()> {
                get_input_file::<File>(l, raw_path.clone())?;
                create_out_file::<FileInput>(l, trans_path.clone())?;
                Ok(())
            })?;
        Ok(())
    }

    /// extracts URLs from lines
    async fn extract(&mut self) -> anyhow::Result<()> {
        let handles = process(
            &mut self.filter_lists,
            &|flist: Arc<FilterList>, chunk: Option<String>| async move {
                if chunk.is_none() {
                    return Ok(None);
                }
                let re = Regex::new(&flist.regex).unwrap();
                if let Some(caps) = re.captures(&chunk.unwrap()) {
                    if let Some(cap) = caps.get(1) {
                        return Ok(Some(cap.as_str().to_owned() + "\n"));
                    }
                }
                Ok(None)
            },
            self.command_rx.clone(),
            self.message_tx.clone(),
        )
        .await;
        join_all(handles).await;
        Ok(())
    }
}

fn get_input_file<W: Write>(
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

fn create_input_urls(list: &mut FilterListIO<UrlInput, File>) -> anyhow::Result<()> {
    let url = Url::parse(&list.filter_list.source)?;
    let input = UrlInput::new(url);
    list.reader = Some(Arc::new(Mutex::new(input)));
    Ok(())
}

fn create_out_file<R: Input>(
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
async fn process<SRC, DST, FN, RES>(
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
