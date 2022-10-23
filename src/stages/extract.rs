use std::{fs::File, marker::PhantomData, path::PathBuf, str::FromStr, sync::Arc};

use futures::future::join_all;
use regex::Regex;

use crate::{
    filter_controller::{
        create_out_file, get_input_file, get_out_file, process, FilterController, StageCategorize,
        StageExtract, RAW_PATH, TRANSFORM_PATH,
    },
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
impl FilterController<StageExtract, FileInput, File> {
    /// Runs the extract stage and returns the controller for the categorize stage
    pub async fn run(
        &mut self,
    ) -> anyhow::Result<FilterController<StageCategorize, FileInput, File>> {
        let mut raw_path = PathBuf::from_str(&self.config.tmp_dir)?;
        raw_path.push(RAW_PATH);
        let mut trans_path = PathBuf::from_str(&self.config.tmp_dir)?;
        trans_path.push(TRANSFORM_PATH);

        self.prepare_extract(raw_path.clone(), trans_path.clone())
            .await?;
        self.extract().await?;
        let categorize_controller = FilterController::<StageCategorize, FileInput, File> {
            stage: PhantomData,
            config: self.config.clone(),
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
        raw_path: PathBuf,
        extract_path: PathBuf,
    ) -> anyhow::Result<()> {
        let configured_lists: Vec<FilterListIO<FileInput, File>> = self
            .config
            .lists
            .iter()
            .map(|f| FilterListIO::new(f.clone()))
            .collect();

        for mut list in configured_lists.into_iter() {
            let compression = list.filter_list.compression.clone();
            get_input_file(&mut list, &raw_path, compression)?;
            get_out_file(&mut list, &extract_path)?;
            if !list.is_cached().await? {
                create_out_file(&mut list, &extract_path)?;
                self.filter_lists.push(list);
            } else {
                debug!("List {} is cached, skipping", list.filter_list.id);
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
    use super::*;

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
