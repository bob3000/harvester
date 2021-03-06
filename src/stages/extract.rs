use std::{fs::File, marker::PhantomData, path::PathBuf, str::FromStr, sync::Arc};

use futures::future::join_all;
use regex::Regex;

use crate::{
    filter_controller::{
        create_out_file, get_input_file, process, FilterController, StageCategorize, StageExtract,
        RAW_PATH, TRANSFORM_PATH,
    },
    filter_list::FilterList,
    input::file::FileInput,
    io::filter_list_io::FilterListIO,
};

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

        self.prepare_extract(raw_path.clone(), trans_path.clone())?;
        self.extract().await?;
        let categorize_controller = FilterController::<StageCategorize, FileInput, File> {
            stage: PhantomData,
            config: self.config.clone(),
            command_rx: self.command_rx.clone(),
            message_tx: self.message_tx.clone(),
            filter_lists: vec![],
            category_lists: vec![],
        };
        Ok(categorize_controller)
    }

    /// Attaches readers and writers to the FilterListIO objects
    ///
    /// * `raw_path`: the file system path to where the downloaded lists were stored
    /// * `extract_path`: the file system path to where the extracted URLs are written to
    fn prepare_extract(&mut self, raw_path: PathBuf, extract_path: PathBuf) -> anyhow::Result<()> {
        self.filter_lists = self
            .config
            .lists
            .iter()
            .map(|f| FilterListIO::new(f.clone()))
            .collect();
        self.filter_lists
            .iter_mut()
            .try_for_each(|l| -> anyhow::Result<()> {
                get_input_file::<File>(l, raw_path.clone())?;
                create_out_file::<FileInput>(l, extract_path.clone())?;
                Ok(())
            })?;
        Ok(())
    }

    /// extracts URLs from lines by employing the regex given in the configuration file
    async fn extract(&mut self) -> anyhow::Result<()> {
        let handles = process(
            &mut self.filter_lists,
            &|flist: Arc<FilterList>, chunk: Option<String>| async move {
                if chunk.is_none() {
                    return Ok(None);
                }
                let re = Regex::new(&flist.regex).unwrap();
                if let Some(caps) = re.captures(&chunk.unwrap()) {
                    if let Some(cap) = caps.get(1) {
                        return Ok(Some(cap.as_str().to_owned() + "\n"));
                    }
                }
                Ok(None)
            },
            self.command_rx.clone(),
            self.message_tx.clone(),
        )
        .await;
        join_all(handles).await;
        Ok(())
    }
}
