use std::{
    fs::File,
    marker::PhantomData,
    path::PathBuf,
    str::FromStr,
    sync::{atomic::AtomicBool, Arc},
};

use flume::Sender;
use futures::future::join_all;

use crate::{
    config::Config,
    filter_controller::{
        create_input_urls, create_out_file, process, ChannelMessage, FilterController,
        StageDownload, StageExtract, RAW_PATH,
    },
    input::{file::FileInput, url::UrlInput},
    io::filter_list_io::FilterListIO,
};

/// This implementation for UrlInput and File is the first phase where the lists
/// are downloaded.
impl FilterController<StageDownload, UrlInput, File> {
    pub fn new(
        config: Config,
        message_tx: Sender<ChannelMessage>,
        is_processing: Arc<AtomicBool>,
    ) -> Self {
        Self {
            stage: PhantomData,
            config,
            message_tx,
            filter_lists: vec![],
            category_lists: vec![],
            is_processing,
        }
    }

    /// Runs the data processing function with UrlInput as input source and a
    /// file as output destination. Returns the controller for the extract stage
    pub async fn run(&mut self) -> anyhow::Result<FilterController<StageExtract, FileInput, File>> {
        let mut raw_path = PathBuf::from_str(&self.config.tmp_dir)?;
        raw_path.push(RAW_PATH);

        self.prepare_download(raw_path.clone())?;
        self.download().await?;
        let extract_controller = FilterController::<StageExtract, FileInput, File> {
            stage: PhantomData,
            config: self.config.clone(),
            message_tx: self.message_tx.clone(),
            filter_lists: vec![],
            category_lists: vec![],
            is_processing: self.is_processing.clone(),
        };
        Ok(extract_controller)
    }

    /// Equips the FilterListIO objects with a reader and writers
    ///
    /// * `raw_path`: the file system path to the directory where the raw lists
    ///               are going to be downloaded
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
            self.message_tx.clone(),
            self.is_processing.clone(),
        )
        .await;
        join_all(handles).await;
        Ok(())
    }
}
