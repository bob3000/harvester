use std::{
    collections::{BTreeSet, HashSet},
    fs::{self, File},
    io::{BufWriter, Write},
    marker::PhantomData,
    path::PathBuf,
    str::FromStr,
    sync::atomic::Ordering,
};

use anyhow::Context;
use futures::future::join_all;
use tokio::task::JoinHandle;

use crate::{
    filter_controller::{
        FilterController, StageCategorize, StageOutput, CATEGORIZE_PATH, TRANSFORM_PATH,
    },
    input::{file::FileInput, Input},
    io::filter_list_io::FilterListIO,
};

/// This stage assembles the category lists from the data extracted in the previous stage
/// A category corresponds to a tag on a list.
impl<'config> FilterController<'config, StageCategorize, FileInput, File> {
    /// runs the categorize stage and return controller for the output stage
    pub async fn run(&mut self) -> anyhow::Result<FilterController<StageOutput, FileInput, File>> {
        let mut extract_path = PathBuf::from_str(&self.config.cache_dir)?;
        extract_path.push(TRANSFORM_PATH);
        let mut categorize_path = PathBuf::from_str(&self.config.cache_dir)?;
        categorize_path.push(CATEGORIZE_PATH);

        self.prepare_categorize(extract_path.clone())?;
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
    fn prepare_categorize(&mut self, extract_path: PathBuf) -> anyhow::Result<()> {
        // repopulate filterlists from config file to include all once more
        self.filter_lists = self
            .config
            .lists
            .iter()
            .map(|f| FilterListIO::new(f.clone()))
            .collect();
        self.filter_lists
            .iter_mut()
            .try_for_each(|l| -> anyhow::Result<()> {
                l.attach_existing_input_file(&extract_path, None)?;
                l.writer = None;
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
        for tag in self.config.get_tags() {
            if !self.is_processing.load(Ordering::SeqCst) {
                return Ok(());
            }

            // include all list into the category which have the currently processed tag attached
            let include_lists: Vec<&mut FilterListIO<FileInput, File>> = self
                .filter_lists
                .iter_mut()
                .filter(|l| l.filter_list.tags.contains(&tag))
                .collect();

            // get the list ids of all included lists
            let include_ids: HashSet<String> = include_lists
                .iter()
                .map(|list| list.filter_list.id.clone())
                .collect();

            // calculate the difference between included lists an cached lists
            let difference: HashSet<&String> = include_ids
                .difference(self.cached_lists.as_ref().unwrap())
                .collect();

            // if the cached_config lists vec and the current config lists vec have the same
            // length no list has been removed since the last run
            if let Some(cached_config) = &self.config.cached_config
            && self.config.lists_with_tag(&tag).len() == cached_config.lists_with_tag(&tag).len()
            {
                // if there is no difference between cached lists and included lists there is no need for action
                if difference.is_empty() {
                    self.cached_lists.as_mut().unwrap().insert(tag.clone());
                    info!("Unchanged: {}", tag.to_string());
                    continue;
                }
            }

            // QUESTION: is there a better data structure to enable concurrent access?
            let mut tree_set: BTreeSet<String> = BTreeSet::new();
            let mut out_path = categorize_path.clone();
            out_path.push(&tag);
            let f = File::create(out_path).with_context(|| "could not create out file")?;
            let mut buf_writer = BufWriter::new(f);

            info!("Updated: {}", tag.to_string());

            // read lines from the included list and insert them into a tree set to remove duplicates
            for incl in include_lists {
                let reader = match incl.reader.as_mut() {
                    Some(r) => r,
                    None => {
                        debug!("reader is None: {}", incl.filter_list.id);
                        continue;
                    }
                };
                reader.lock().await.reset().await?;
                while let Ok(Some(chunk)) = reader.lock().await.chunk().await {
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

            // do some sanitizing - empty lines ain't domains
            tree_set.remove("\n");
            tree_set.remove("");
            tree_set.remove(" ");
            tree_set.remove("\t");

            let handle = tokio::spawn(async move {
                for line in tree_set {
                    if let Err(e) = buf_writer.write_all(line.as_bytes()) {
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
