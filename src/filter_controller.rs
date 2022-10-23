use std::{
    fs::{self, File},
    io::Write,
    marker::PhantomData,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use anyhow::Context;
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
    base_dir: &Path,
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
    let url = Url::parse(&list.filter_list.source)
        .with_context(|| format!("config file error: {:?}", &list.filter_list))?;
    let input = UrlInput::new(url);
    list.reader = Some(Arc::new(Mutex::new(input)));
    Ok(())
}

/// Tries to read the potential output file for inspection
///
/// * `list`: the FilterListIO object to receive the writer
/// * `base_dir`: the base directory where the output file is tried to read
pub fn get_out_file<R: Input + Send>(
    list: &mut FilterListIO<R, File>,
    base_dir: &Path,
) -> anyhow::Result<()> {
    let out_path = base_dir;
    let mut out_path = out_path.to_path_buf();
    out_path.push(&list.filter_list.id);
    let out_file = File::open(out_path).with_context(|| "could not open out file for reading")?;
    list.writer = Some(Arc::new(Mutex::new(out_file)));
    Ok(())
}

/// Creates and output file and it's parent directories, opens the file for writing
/// and attaches it to the given FilterListIO object
///
/// * `list`: the FilterListIO object to receive the writer
/// * `base_dir`: the base directory where the output file is being created
pub fn create_out_file<R: Input + Send>(
    list: &mut FilterListIO<R, File>,
    base_dir: &Path,
) -> anyhow::Result<()> {
    let mut out_path = base_dir.to_path_buf();
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

        info!("{}: {}", filter_list.id, filter_list.source);
        let is_proc = Arc::clone(&is_processing);
        let handle = tokio::spawn(async move {
            let mut chunks_matched = 0;
            let mut chunks_skipped = 0;
            loop {
                if !is_proc.load(Ordering::SeqCst) {
                    debug!("quitting task: {}", list.id);
                    return;
                }
                // stop task on quit message
                let result = reader.lock().await.chunk().await;
                match result {
                    Ok(Some(chunk)) => match fn_transform(list.clone(), Some(chunk)).await {
                        // regex matched
                        Ok(Some(chunk)) => {
                            chunks_matched += 1;
                            if let Err(e) = writer.lock().await.write_all(&chunk) {
                                error!("{}", e);
                            }
                        }
                        // regex did not match
                        Ok(None) => {
                            chunks_skipped += 1;
                        }
                        // regex error
                        Err(e) => {
                            error!("Error: {}", e);
                            break;
                        }
                    },
                    // reader exhausted
                    Ok(None) => {
                        break;
                    }
                    // reader error
                    Err(e) => {
                        error!("Error: {}", e);
                        break;
                    }
                }
            }
            if chunks_matched == 0 {
                warn!("No lines machted in list {}", list.id);
            } else {
                debug!("{}: {} lines matched", list.id, chunks_matched);
                debug!("{}: {} lines skipped", list.id, chunks_skipped);
            }
        });
        handles.push(handle);
    }
    handles
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use crate::filter_list::FilterList;
    use crate::tests::helper::cursor_input::CursorInput;
    use futures::future::join_all;

    use super::*;

    /// tests the `process` function using the TestInput to avoid writing files
    #[tokio::test]
    async fn test_process() {
        // create input data
        let input_data = "line one\nline two\n";
        let input = Arc::new(Mutex::new(CursorInput::new(input_data)));
        // set up output sink
        let output = Arc::new(Mutex::new(Cursor::new(vec![0, 32])));
        let is_processing = Arc::new(AtomicBool::new(true));

        // apply the data to the FilterList object
        let filter_list = FilterList {
            id: "".to_string(),
            compression: None,
            comment: None,
            source: "".to_string(),
            tags: vec![],
            regex: "".to_string(),
        };

        // wrap the Filterlist in the FilterListIO object
        let mut filter_list_io: FilterListIO<CursorInput, Cursor<Vec<u8>>> =
            FilterListIO::new(filter_list);
        filter_list_io.reader = Some(input);
        filter_list_io.writer = Some(output.clone());

        // process the data with a transform function just forwarding the data
        let handles = process(
            &mut vec![filter_list_io],
            &|_, c| async { Ok(c) },
            is_processing.clone(),
        )
        .await;
        join_all(handles).await;
        let o = output.lock().await.clone().into_inner();

        // the data in the out put should be the same as the input data
        assert!(String::from_utf8_lossy(&o).starts_with(&input_data));
    }
}
