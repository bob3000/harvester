use std::{
    collections::HashSet,
    fs::File,
    marker::PhantomData,
    path::PathBuf,
    str::FromStr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use futures::future::join_all;

use crate::{
    config::Config,
    filter_controller::{process, FilterController, StageDownload, StageExtract},
    input::{file::FileInput, url::UrlInput},
    io::filter_list_io::FilterListIO,
};

/// This implementation for UrlInput and File is the first phase where the lists
/// are downloaded.
impl<'config> FilterController<'config, StageDownload, UrlInput, File> {
    pub fn new(config: &'config Config, is_processing: Arc<AtomicBool>) -> Self {
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
    ///
    /// * `download_base_path`: target path for files being downloaded
    pub async fn run(
        &mut self,
        download_base_path: &str,
    ) -> anyhow::Result<FilterController<StageExtract, FileInput, File>> {
        let mut download_path = PathBuf::from_str(&self.config.cache_dir)?;
        download_path.push(download_base_path);

        self.prepare_download(download_path.clone()).await?;
        self.download().await?;
        let extract_controller = FilterController::<StageExtract, FileInput, File> {
            stage: PhantomData,
            cached_lists: self.cached_lists.take(),
            config: self.config,
            filter_lists: vec![],
            category_lists: vec![],
            is_processing: self.is_processing.clone(),
        };
        Ok(extract_controller)
    }

    /// Equips the FilterListIO objects with a reader and writers
    ///
    /// * `download_path`: the file system path to the directory where the raw lists
    ///               are going to be downloaded
    async fn prepare_download(&mut self, download_path: PathBuf) -> anyhow::Result<()> {
        let configured_lists: Vec<FilterListIO<UrlInput, File>> = self
            .config
            .lists
            .iter()
            .map(|f| FilterListIO::new(f.clone()))
            .collect();

        for mut list in configured_lists.into_iter() {
            if !self.is_processing.load(Ordering::SeqCst) {
                return Ok(());
            }

            list.attach_url_reader()?;

            let mut is_cached = false;
            // we can only check for a cached result if the former downloaded file is available
            if list.attach_existing_file_writer(&download_path).is_ok() {
                is_cached = list.is_cached().await?;
            }
            if !is_cached {
                info!("Updated: {}", list.filter_list.id);
                list.attach_new_file_writer(&download_path)?;
                self.filter_lists.push(list);
            } else {
                info!("Unchanged: {}", list.filter_list.id);
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
