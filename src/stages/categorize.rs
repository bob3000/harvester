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
    filter_controller::{FilterController, StageCategorize, StageOutput},
    input::{file::FileInput, Input},
    io::{category_list_io::CategoryListIO, filter_list_io::FilterListIO},
};

/// This stage assembles the category lists from the data extracted in the previous stage
/// A category corresponds to a tag on a list.
impl<'config> FilterController<'config, StageCategorize, FileInput, File> {
    /// runs the categorize stage and return controller for the output stage
    ///
    /// * `extract_base_path`: The source path containing the URL lists
    /// * `categorize_base_path`: The target path for the categorized URL lists
    pub async fn run(
        &mut self,
        extract_base_path: &str,
        categorize_base_path: &str,
    ) -> anyhow::Result<FilterController<StageOutput, FileInput, File>> {
        let mut extract_path = PathBuf::from_str(&self.config.cache_dir)?;
        extract_path.push(extract_base_path);
        let mut categorize_path = PathBuf::from_str(&self.config.cache_dir)?;
        categorize_path.push(categorize_base_path);

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
    /// * `extract_path`: The directory where the extracted data from the previous stage was stored
    /// * `categorize_path`: The directory wehre the results of this stage will be stored
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
            for filter_list_io in category_list.included_filter_lists.iter_mut() {
                let flist = match filter_list_io.reader.as_mut() {
                    Some(l) => l,
                    None => {
                        warn!(
                            "filter list {} has no reader attached",
                            filter_list_io.filter_list.id
                        );
                        continue;
                    }
                };
                while let Ok(Some(chunk)) = flist.lock().await.chunk().await {
                    // insert the URLs into a BTreeSet to deduplicate and sort the data
                    let str_chunk = match String::from_utf8(chunk) {
                        Ok(s) => s.trim().to_string(),
                        Err(e) => {
                            warn!("{}", e);
                            continue;
                        }
                    };
                    if str_chunk.is_empty() {
                        continue;
                    }
                    tree_set.insert(str_chunk);
                }
            }

            let writer = category_list.writer.take().unwrap();
            let handle = tokio::spawn(async move {
                for mut line in tree_set {
                    if !line.ends_with('\n') {
                        line.push('\n');
                    }
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

#[cfg(test)]
mod tests {

    use std::{
        collections::HashMap,
        sync::{atomic::AtomicBool, Arc},
    };

    use crate::{
        filter_list::FilterList, tests::helper::cache_file_creator::CacheFileCreator,
        CATEGORIZE_PATH, EXTRACT_PATH,
    };

    use super::*;

    #[tokio::test]
    async fn test_categorize_successful() {
        // prepare folder structure
        let cache =
            CacheFileCreator::new("test_categorize_successful", EXTRACT_PATH, CATEGORIZE_PATH);
        let mut config = cache.new_test_config();
        // three lists tagged with: advertising, malware and advertising + malware
        config.lists = vec![
            FilterList {
                id: "advertising".to_string(),
                comment: None,
                compression: None,
                source: "".to_string(),
                tags: vec!["advertising".to_string()],
                regex: r"(.*)".to_string(),
            },
            FilterList {
                id: "malware".to_string(),
                comment: None,
                compression: None,
                source: "".to_string(),
                tags: vec!["malware".to_string()],
                regex: r"(.*)".to_string(),
            },
            FilterList {
                id: "advertising_malware".to_string(),
                comment: None,
                compression: None,
                source: "".to_string(),
                tags: vec!["malware".to_string(), "advertising".to_string()],
                regex: r"(.*)".to_string(),
            },
        ];
        // the contents of each filter list
        let contents = vec![
            vec!["one.domain", "another.domain"].join("\n"),
            vec!["third.domain", "fourth.domain"].join("\n"),
            vec!["fith.domain", "sixth.domain"].join("\n"),
        ];
        for i in 0..=2 {
            cache.write_input(&config.lists[i].id, &contents[i]);
        }

        let mut categorize_controller = FilterController::<StageCategorize, FileInput, File> {
            stage: PhantomData,
            cached_lists: Some(HashSet::new()),
            config: &config,
            filter_lists: vec![],
            category_lists: vec![],
            is_processing: Arc::new(AtomicBool::new(true)),
        };
        if let Err(e) = categorize_controller
            .run(&cache.inpath, &cache.outpath)
            .await
        {
            error!("{}", e);
        }

        // the advertising list is expected to have the contents from list 0 and 2
        let mut advertising = vec![
            contents[0].split("\n").collect::<Vec<&str>>(),
            contents[2].split("\n").collect::<Vec<&str>>(),
        ]
        .concat();
        // the malware list is expected to have the contents from list 1 and 2
        let mut malware = vec![
            contents[1].split("\n").collect::<Vec<&str>>(),
            contents[2].split("\n").collect::<Vec<&str>>(),
        ]
        .concat();
        advertising.sort();
        malware.sort();
        let want = HashMap::from([
            ("advertising", advertising.join("\n") + "\n"),
            ("malware", malware.join("\n") + "\n"),
        ]);
        // read from files written and compare results for each category list
        for category in vec!["advertising", "malware"] {
            let got = cache.read_result(&category).unwrap();
            let want = want.get(&category).unwrap();
            assert_eq!(want, &got);
        }
    }
}
