use std::{fs::File, marker::PhantomData, path::PathBuf, str::FromStr, sync::Arc};

use futures::future::join_all;
use regex::Regex;

use crate::{
    filter_controller::{process, FilterController, StageCategorize, StageExtract},
    filter_list::FilterList,
    input::file::FileInput,
    io::filter_list_io::FilterListIO,
};

async fn regex_match(
    flist: Arc<FilterList>,
    chunk: Option<Vec<u8>>,
) -> anyhow::Result<Option<Vec<u8>>> {
    if chunk.is_none() {
        return Ok(None);
    }
    let str_chunk = match String::from_utf8(chunk.unwrap()) {
        Ok(s) => s,
        Err(e) => {
            return Err(anyhow::anyhow!("Error: {}", e));
        }
    };
    let re = match Regex::new(&flist.regex) {
        Ok(r) => r,
        Err(e) => return Err(anyhow::anyhow!(format!("List {} - {}", flist.id, e))),
    };
    if let Some(caps) = re.captures(&str_chunk) && let Some(cap) = caps.get(1) {
                    let result = cap.as_str().to_owned() + "\n";
                    return Ok(Some(result.as_bytes().to_owned()));
                }
    Ok(None)
}

/// This implementation for FileInput and File is the second stage where URLs are
/// being extracted
impl<'config> FilterController<'config, StageExtract, FileInput, File> {
    /// Runs the extract stage and returns the controller for the categorize stage
    pub async fn run(
        &mut self,
        download_base_path: &str,
        extract_base_path: &str,
    ) -> anyhow::Result<FilterController<StageCategorize, FileInput, File>> {
        let mut download_path = PathBuf::from_str(&self.config.cache_dir)?;
        download_path.push(download_base_path);
        let mut extract_path = PathBuf::from_str(&self.config.cache_dir)?;
        extract_path.push(extract_base_path);

        self.prepare_extract(download_path.clone(), extract_path.clone())
            .await?;
        self.extract().await?;
        let categorize_controller = FilterController::<StageCategorize, FileInput, File> {
            stage: PhantomData,
            config: self.config,
            cached_lists: self.cached_lists.take(),
            filter_lists: vec![],
            category_lists: vec![],
            is_processing: self.is_processing.clone(),
        };
        Ok(categorize_controller)
    }

    /// Attaches readers and writers to the FilterListIO objects
    ///
    /// * `raw_path`: the file system path to where the downloaded lists were stored
    /// * `extract_path`: the file system path to where the extracted URLs are written to
    async fn prepare_extract(
        &mut self,
        download_path: PathBuf,
        extract_path: PathBuf,
    ) -> anyhow::Result<()> {
        let configured_lists: Vec<FilterListIO<FileInput, File>> = self
            .config
            .lists
            .iter()
            .map(|f| FilterListIO::new(f.clone()))
            .collect();

        for mut list in configured_lists {
            if self
                .cached_lists
                .as_ref()
                .unwrap()
                .contains(&list.filter_list.id)
                && list
                    .attach_existing_input_file(&download_path, None)
                    .is_ok()
                && list.attach_existing_file_writer(&extract_path).is_ok()
            {
                list.writer = None;
                info!("Unchanged: {}", list.filter_list.id);
            } else {
                self.cached_lists
                    .as_mut()
                    .unwrap()
                    .retain(|l| l != &list.filter_list.id);
                info!("Updated: {}", list.filter_list.id);
                let compression = list.filter_list.compression.clone();
                list.attach_existing_input_file(&download_path, compression)?;
                list.attach_new_file_writer(&extract_path)?;
                self.filter_lists.push(list);
            }
        }
        Ok(())
    }

    /// extracts URLs from lines by employing the regex given in the configuration file
    async fn extract(&mut self) -> anyhow::Result<()> {
        let handles = process(
            &mut self.filter_lists,
            &regex_match,
            self.is_processing.clone(),
        )
        .await;
        join_all(handles).await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, sync::atomic::AtomicBool};

    use crate::{tests::helper::cache_file_creator::CacheFileCreator, DOWNLOAD_PATH, EXTRACT_PATH};

    use super::*;

    #[tokio::test]
    async fn test_extract_successful() {
        let cache = CacheFileCreator::new(DOWNLOAD_PATH, EXTRACT_PATH);
        let mut config = cache.new_test_config();
        config.lists = vec![FilterList {
            id: "test".to_string(),
            comment: None,
            compression: None,
            source: "".to_string(),
            tags: vec![],
            // the regex for matching lines
            regex: r"127.0.0.1 (.*)".to_string(),
        }];
        // prepare the file to extract from
        cache.write_input(
            &config.lists[0].id,
            r#"
127.0.0.1 one.domain
127.0.0.1 another.domain
"#,
        );

        let mut extract_controller = FilterController::<StageExtract, FileInput, File> {
            stage: PhantomData,
            cached_lists: Some(HashSet::new()),
            config: &config,
            filter_lists: vec![],
            category_lists: vec![],
            is_processing: Arc::new(AtomicBool::new(true)),
        };
        if let Err(e) = extract_controller.run(&cache.inpath, &cache.outpath).await {
            error!("{}", e);
        }
        // we expect the result file only to contain the domains according to the regex
        let want = r#"one.domain
another.domain
"#;
        let got = cache.read_result(&config.lists[0].id).unwrap();
        assert_eq!(want, got);
    }

    #[tokio::test]
    async fn test_regex_match_positive() {
        let regex = "^0.0.0.0 (.*)".to_string();
        let filter_list = FilterList {
            id: "test_list".to_string(),
            compression: None,
            comment: None,
            source: "".to_string(),
            tags: vec![],
            regex,
        };
        let chunk = Vec::from("0.0.0.0 domain.tech\n");

        let got = regex_match(Arc::new(filter_list), Some(chunk))
            .await
            .unwrap()
            .unwrap();
        let want = Vec::from("domain.tech\n");

        assert_eq!(got, want);
    }

    #[tokio::test]
    async fn test_regex_no_match_comment() {
        let regex = "^0.0.0.0 (.*)".to_string();
        let filter_list = FilterList {
            id: "test_list".to_string(),
            compression: None,
            comment: None,
            source: "".to_string(),
            tags: vec![],
            regex,
        };
        let chunk = Vec::from("# some comment\n");

        let got = regex_match(Arc::new(filter_list), Some(chunk))
            .await
            .unwrap();
        let want: Option<Vec<u8>> = None;

        assert_eq!(got, want);
    }
}
