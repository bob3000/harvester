use std::{
    collections::HashSet,
    fs::File,
    marker::PhantomData,
    path::PathBuf,
    str::FromStr,
    sync::{atomic::AtomicBool, Arc},
};

use futures::future::join_all;

use crate::{
    config::Config,
    filter_controller::{
        create_input_urls, create_out_file, get_out_file, process, FilterController, StageDownload,
        StageExtract, RAW_PATH,
    },
    input::{file::FileInput, url::UrlInput},
    io::filter_list_io::FilterListIO,
};

/// This implementation for UrlInput and File is the first phase where the lists
/// are downloaded.
impl FilterController<StageDownload, UrlInput, File> {
    pub fn new(config: Config, is_processing: Arc<AtomicBool>) -> Self {
        Self {
            stage: PhantomData,
            config,
            cached_lists: Some(HashSet::new()),
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

        self.prepare_download(raw_path.clone()).await?;
        self.download().await?;
        let extract_controller = FilterController::<StageExtract, FileInput, File> {
            stage: PhantomData,
            cached_lists: self.cached_lists.take(),
            config: self.config.clone(),
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
    async fn prepare_download(&mut self, raw_path: PathBuf) -> anyhow::Result<()> {
        let configured_lists: Vec<FilterListIO<UrlInput, File>> = self
            .config
            .lists
            .iter()
            .map(|f| FilterListIO::new(f.clone()))
            .collect();

        for mut list in configured_lists.into_iter() {
            create_input_urls(&mut list)?;
            get_out_file(&mut list, &raw_path)?;
            if !list.is_cached().await? {
                create_out_file(&mut list, &raw_path)?;
                self.filter_lists.push(list);
            } else {
                info!("List {} is cached, skipping", list.filter_list.id);
                self.cached_lists
                    .as_mut()
                    .unwrap()
                    .insert(list.filter_list.id);
            }
        }
        Ok(())
    }

    /// downloads lists to temp files
    async fn download(&mut self) -> anyhow::Result<()> {
        let handles = process(
            &mut self.filter_lists,
            &|_, chunk| async { Ok(chunk) },
            self.is_processing.clone(),
        )
        .await;
        join_all(handles).await;
        Ok(())
    }
}
