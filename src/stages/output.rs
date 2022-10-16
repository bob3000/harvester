use std::{
    fs::{self, File},
    path::PathBuf,
    str::FromStr,
    sync::{atomic::Ordering, Arc},
};

use anyhow::Context;
use futures::{future::join_all, lock::Mutex};
use tokio::task::JoinHandle;

use crate::{
    filter_controller::{FilterController, StageOutput, CATEGORIZE_PATH},
    input::file::FileInput,
    io::category_list_io::CategoryListIO,
};

impl FilterController<StageOutput, FileInput, File> {
    /// Runs the output stage
    pub async fn run(&mut self) -> anyhow::Result<()> {
        let mut categorize_path = PathBuf::from_str(&self.config.tmp_dir)?;
        categorize_path.push(CATEGORIZE_PATH);
        let out_path = PathBuf::from_str(&self.config.out_dir)?;

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
                let mut contents = fs::read_dir(&categorize_path)
                    .with_context(|| "input file directory does not exist")?;
                let entry = contents
                    .find(|it| {
                        if let Ok(it) = it {
                            return it.file_name().to_str().unwrap() == list.name;
                        }
                        false
                    })
                    .ok_or_else(|| anyhow::anyhow!("file not found: {}", list.name))??;
                list.reader = Some(Arc::new(Mutex::new(FileInput::new(entry.path(), None))));

                // set writers
                let mut out_path = output_path.clone();
                fs::create_dir_all(&output_path)
                    .with_context(|| "could not create out directory")?;
                out_path.push(&list.name);
                let out_file =
                    File::create(out_path).with_context(|| "could not write out file")?;
                list.writer = Some(Arc::new(Mutex::new(out_file)));
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
            info!("{}", list.name);
            let reader = Arc::clone(&list.reader.take().unwrap());
            let writer = Arc::clone(&list.writer.take().unwrap());
            let output_adapter =
                self.config
                    .out_format
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
