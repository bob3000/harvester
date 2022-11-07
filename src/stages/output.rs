use std::{
    fs::File,
    path::PathBuf,
    str::FromStr,
    sync::{atomic::Ordering, Arc},
};

use futures::future::join_all;
use tokio::task::JoinHandle;

use crate::{
    filter_controller::{FilterController, StageOutput},
    input::file::FileInput,
    io::category_list_io::CategoryListIO,
};

impl<'config> FilterController<'config, StageOutput, FileInput, File> {
    /// Runs the output stage
    pub async fn run(&mut self, categorize_base_path: &str) -> anyhow::Result<()> {
        let mut categorize_path = PathBuf::from_str(&self.config.cache_dir)?;
        categorize_path.push(categorize_base_path);
        let out_path = PathBuf::from_str(&self.config.output_dir)?;

        self.prepare_output(categorize_path.clone(), out_path)?;
        self.output().await?;
        Ok(())
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
            .config
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

#[cfg(test)]
mod tests {

    use std::{
        collections::{HashMap, HashSet},
        marker::PhantomData,
        sync::{atomic::AtomicBool, Arc},
    };

    use crate::{
        filter_list::FilterList, tests::helper::cache_file_creator::CacheFileCreator,
        CATEGORIZE_PATH,
    };

    use super::*;

    #[tokio::test]
    async fn test_output_successful() {
        // prepare folder structure
        let cache = CacheFileCreator::new("test_output_successful", CATEGORIZE_PATH, "output");
        let mut config = cache.new_test_config();
        // for output to work we need these FilterLists for the tags to be present in the config
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
        ];
        // the contents of each filter list
        let contents: HashMap<&str, String> = HashMap::from([
            (
                "advertising",
                vec![
                    "another.domain",
                    "fith.domain",
                    "one.domain",
                    "sixth.domain",
                ]
                .join("\n")
                    + "\n",
            ),
            (
                "malware",
                vec![
                    "fith.domain",
                    "fourth.domain",
                    "sixth.domain",
                    "third.domain",
                ]
                .join("\n")
                    + "\n",
            ),
        ]);

        for (list, content) in &contents {
            cache.write_input(list, &content);
        }

        let mut output_controller = FilterController::<StageOutput, FileInput, File> {
            stage: PhantomData,
            cached_lists: Some(HashSet::new()),
            config: &config,
            filter_lists: vec![],
            category_lists: vec![],
            is_processing: Arc::new(AtomicBool::new(true)),
        };
        if let Err(e) = output_controller.run(&cache.inpath).await {
            error!("{}", e);
        }

        let mut want = HashMap::new();
        for category in vec!["advertising", "malware"] {
            let cont = &contents.get(&category).unwrap();
            let cont_str = cont
                .trim_end()
                .split("\n")
                .map(|line| format!("0.0.0.0 {}", line.trim()))
                .collect::<Vec<String>>()
                .join("\n")
                + "\n";
            want.insert(category, cont_str).unwrap_or_default();
        }
        // read from files written and compare results for each category list
        for category in vec!["advertising", "malware"] {
            let got = cache.read_result(&category).unwrap();
            let want = want.get(&category).unwrap();
            assert_eq!(want, &got);
        }
    }
}
