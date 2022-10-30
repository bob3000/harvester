use std::{
    fs::File,
    path::PathBuf,
    str::FromStr,
    sync::{atomic::Ordering, Arc},
};

use futures::future::join_all;
use tokio::task::JoinHandle;

use crate::{
    filter_controller::{FilterController, StageOutput, CATEGORIZE_PATH},
    input::file::FileInput,
    io::category_list_io::CategoryListIO,
};

impl<'config> FilterController<'config, StageOutput, FileInput, File> {
    /// Runs the output stage
    pub async fn run(&mut self) -> anyhow::Result<()> {
        let mut categorize_path = PathBuf::from_str(&self.config.cache_dir)?;
        categorize_path.push(CATEGORIZE_PATH);
        let out_path = PathBuf::from_str(&self.config.output_dir)?;

        self.prepare_output(categorize_path.clone(), out_path)?;
        self.output().await?;
        Ok(())
    }

    /// returns a list of all existing tags taken from the configuration file
    fn get_tags(&self) -> Vec<String> {
        let mut tags: Vec<String> = Vec::new();
        for list in self.config.lists.iter() {
            list.tags.iter().for_each(|t| {
                if !tags.contains(t) {
                    tags.push(t.clone())
                }
            });
        }
        tags
    }

    /// Attaches the readers and writers to the CategoryListIO objects
    ///
    /// * `categorize_path`: the file system path to where the category lists where stored
    /// * `output_path`: the file system path for the lists in the final result format
    fn prepare_output(
        &mut self,
        categorize_path: PathBuf,
        output_path: PathBuf,
    ) -> anyhow::Result<()> {
        self.category_lists = self
            .get_tags()
            .iter()
            .map(|t| CategoryListIO::new(&t.clone()))
            .collect();
        self.category_lists
            .iter_mut()
            .try_for_each(|list| -> anyhow::Result<()> {
                // set readers
                list.attach_existing_input_file(&categorize_path)?;

                // set writers
                if self.cached_lists.as_ref().unwrap().contains(&list.name)
                    && list.attach_existing_input_file(&categorize_path).is_ok()
                    && list.attach_existing_file_writer(&output_path).is_ok()
                {
                    // set writer to None so it will be skipped in the output method
                    list.writer = None;
                    return Ok(());
                }
                list.attach_new_file_writer(&output_path)?;
                Ok(())
            })?;
        Ok(())
    }

    /// generates the final result lists
    async fn output(&mut self) -> anyhow::Result<()> {
        let mut handles: Vec<JoinHandle<()>> = vec![];
        for list in self.category_lists.iter_mut() {
            if !self.is_processing.load(Ordering::SeqCst) {
                return Ok(());
            }
            // do nothing if the list was already written on the last run
            if self.cached_lists.as_ref().unwrap().contains(&list.name) && list.writer.is_none() {
                info!("Unchanged: {}", list.name);
                continue;
            }
            info!("Updated: {}", list.name);
            let reader = Arc::clone(&list.reader.take().unwrap());
            let writer = Arc::clone(&list.writer.take().unwrap());
            let output_adapter =
                self.config
                    .output_format
                    .get_adapter(reader, writer, self.is_processing.clone());
            let handle = tokio::spawn(async move {
                output_adapter.await;
            });
            handles.push(handle);
        }
        join_all(handles).await;
        Ok(())
    }
}
