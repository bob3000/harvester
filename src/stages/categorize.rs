use std::{
    collections::{BTreeSet, HashSet},
    fs::{self, File},
    io::Write,
    marker::PhantomData,
    path::{Path, PathBuf},
    str::FromStr,
    sync::atomic::Ordering,
};

use anyhow::Context;
use futures::future::join_all;
use tokio::task::JoinHandle;

use crate::{
    filter_controller::{
        FilterController, StageCategorize, StageOutput, CATEGORIZE_PATH, EXTRACT_PATH,
    },
    input::{file::FileInput, Input},
    io::{category_list_io::CategoryListIO, filter_list_io::FilterListIO},
};

/// This stage assembles the category lists from the data extracted in the previous stage
/// A category corresponds to a tag on a list.
impl<'config> FilterController<'config, StageCategorize, FileInput, File> {
    /// runs the categorize stage and return controller for the output stage
    pub async fn run(&mut self) -> anyhow::Result<FilterController<StageOutput, FileInput, File>> {
        let mut extract_path = PathBuf::from_str(&self.config.cache_dir)?;
        extract_path.push(EXTRACT_PATH);
        let mut categorize_path = PathBuf::from_str(&self.config.cache_dir)?;
        categorize_path.push(CATEGORIZE_PATH);

        self.prepare_categorize(&extract_path, &categorize_path)?;
        self.categorize(categorize_path).await?;
        let output_controller = FilterController::<StageOutput, FileInput, File> {
            stage: PhantomData,
            config: self.config,
            cached_lists: self.cached_lists.take(),
            filter_lists: vec![],
            category_lists: vec![],
            is_processing: self.is_processing.clone(),
        };
        Ok(output_controller)
    }

    /// Attaches the source file reader to the FilterListIO
    ///
    /// * `extract_path`: The directory where the extracted data from the previous
    ///                   step was stored
    fn prepare_categorize(
        &mut self,
        extract_path: &Path,
        categorize_path: &Path,
    ) -> anyhow::Result<()> {
        // prepare category lists for writing
        self.config
            .get_tags()
            .iter()
            .try_for_each(|tag| -> anyhow::Result<()> {
                let mut category_list = CategoryListIO::new(tag);
                let included_lists = self.config.lists_with_tag(tag);

                // include all ids into the category which have the currently processed tag attached
                let include_ids: HashSet<String> = self
                    .config
                    .lists_with_tag(tag)
                    .iter()
                    .map(|list| list.id.clone())
                    .collect();

               // calculate the difference between included lists an cached lists
                let difference: HashSet<&String> = include_ids
                    .difference(self.cached_lists.as_ref().unwrap())
                    .collect();

                // if the cached_config lists vec and the current config lists vec have the same
                // length no list has been removed since the last run
                if let Some(cached_config) = &self.config.cached_config
                    && self.config.lists_with_tag(tag).len() == cached_config.lists_with_tag(tag).len()
                    // if there is no difference between cached lists and included lists there is no need for action
                    && difference.is_empty()
                    // check if there was actually a file written on the last run
                    && category_list.attach_existing_file_writer(categorize_path).is_ok()
                {
                    self.cached_lists.as_mut().unwrap().insert(tag.clone());
                    category_list.writer = None;
                    info!("Unchanged: {}", tag.to_string());
                    return Ok(());
                }

                category_list.attach_new_file_writer(categorize_path)?;
                category_list.included_filter_lists = included_lists.into_iter().filter_map(|flist| {
                    let mut flist_io = FilterListIO::new(flist.to_owned());
                    if let Err(e) = flist_io.attach_existing_input_file(extract_path, None) {
                        error!("Error: {} - {}", flist_io.filter_list.id, e);
                        return None;
                    }
                    Some(flist_io)
                }).collect();

                self.category_lists.push(category_list);
                Ok(())
            })?;
        Ok(())
    }

    /// assembles the category lists from the extracted URLs according to the existing tags
    /// in the configuration file
    ///
    /// * `categorize_path`: the file system path where the resulting lists are stored
    async fn categorize(&mut self, categorize_path: PathBuf) -> anyhow::Result<()> {
        fs::create_dir_all(&categorize_path).with_context(|| "could not create out directory")?;
        let mut handles: Vec<JoinHandle<()>> = vec![];
        for category_list in self.category_lists.iter_mut() {
            if !self.is_processing.load(Ordering::SeqCst) {
                return Ok(());
            }

            // QUESTION: is there a better data structure to enable concurrent access?
            let mut tree_set: BTreeSet<String> = BTreeSet::new();

            info!("Updated: {}", category_list.name);

            // read lines from the included list and insert them into a tree set to remove duplicates
            for filter_list in category_list.included_filter_lists.iter_mut() {
                while let Ok(Some(chunk)) = filter_list
                    .reader
                    .as_mut()
                    .unwrap()
                    .lock()
                    .await
                    .chunk()
                    .await
                {
                    // insert the URLs into a BTreeSet to deduplicate and sort the data
                    let str_chunk = match String::from_utf8(chunk) {
                        Ok(s) => s,
                        Err(e) => {
                            anyhow::anyhow!("{}", e);
                            continue;
                        }
                    };
                    tree_set.insert(str_chunk);
                }
            }

            // TODO: write test. It that actually helpful?
            // do some sanitizing - empty lines ain't domains
            tree_set.remove("\n");
            tree_set.remove("");
            tree_set.remove(" ");
            tree_set.remove("\t");

            let writer = category_list.writer.take().unwrap();
            let handle = tokio::spawn(async move {
                for line in tree_set {
                    if let Err(e) = writer.lock().await.write_all(line.as_bytes()) {
                        error!("{:?}", e);
                        break;
                    }
                }
            });
            handles.push(handle);
        }
        join_all(handles).await;
        Ok(())
    }
}
