use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::{BufWriter, Write},
    marker::PhantomData,
    path::PathBuf,
    str::FromStr,
};

use anyhow::Context;
use futures::future::join_all;
use tokio::task::JoinHandle;

use crate::{
    filter_controller::{
        get_input_file, ChannelCommand, ChannelMessage, FilterController, StageCategorize,
        StageOutput, CATEGORIZE_PATH, TRANSFORM_PATH,
    },
    input::{file::FileInput, Input},
    io::filter_list_io::FilterListIO,
};

/// This stage assembles the category lists from the data extracted in the previous stage
/// A category corresponds to a tag on a list.
impl FilterController<StageCategorize, FileInput, File> {
    /// runs the categorize stage and return controller for the output stage
    pub async fn run(&mut self) -> anyhow::Result<FilterController<StageOutput, FileInput, File>> {
        let mut extract_path = PathBuf::from_str(&self.config.tmp_dir)?;
        extract_path.push(TRANSFORM_PATH);
        let mut categorize_path = PathBuf::from_str(&self.config.tmp_dir)?;
        categorize_path.push(CATEGORIZE_PATH);

        self.prepare_categorize(extract_path.clone())?;
        self.categorize(categorize_path).await?;
        let categorize_controller = FilterController::<StageOutput, FileInput, File> {
            stage: PhantomData,
            config: self.config.clone(),
            command_rx: self.command_rx.clone(),
            message_tx: self.message_tx.clone(),
            filter_lists: vec![],
            category_lists: vec![],
        };
        Ok(categorize_controller)
    }

    /// extracts all existing tags from the filter list configuration
    fn get_tags(&self) -> Vec<String> {
        let mut tags: Vec<String> = Vec::new();
        for list_io in self.filter_lists.iter() {
            list_io.filter_list.tags.iter().for_each(|t| {
                if !tags.contains(t) {
                    tags.push(t.clone())
                }
            });
        }
        tags
    }

    /// Attaches the source file reader to the FilterListIO
    ///
    /// * `extract_path`: The directory where the extracted data from the previous
    ///                   step was stored
    fn prepare_categorize(&mut self, extract_path: PathBuf) -> anyhow::Result<()> {
        self.filter_lists = self
            .config
            .lists
            .iter()
            .map(|f| FilterListIO::new(f.clone()))
            .collect();
        self.filter_lists
            .iter_mut()
            .try_for_each(|l| -> anyhow::Result<()> {
                get_input_file::<File>(l, extract_path.clone(), None)?;
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
        for tag in self.get_tags() {
            let mut tree_set: BTreeSet<String> = BTreeSet::new();
            let mut out_path = categorize_path.clone();
            out_path.push(&tag);
            let f = File::create(out_path).with_context(|| "could not create out file")?;
            let mut buf_writer = BufWriter::new(f);
            let cmd_rx = self.command_rx.clone();
            let msg_tx = self.message_tx.clone();

            msg_tx
                .send(ChannelMessage::Info(format!("{}", tag)))
                .unwrap();

            let include_lists = self
                .filter_lists
                .iter_mut()
                .filter(|l| l.filter_list.tags.contains(&tag));

            for incl in include_lists {
                incl.reader.as_mut().unwrap().lock().await.reset().await?;
                while let Ok(Some(chunk)) = incl.reader.as_mut().unwrap().lock().await.chunk().await
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

            let handle = tokio::spawn(async move {
                for line in tree_set {
                    // stop task on quit message
                    if let Ok(cmd) = cmd_rx.try_recv() {
                        match cmd {
                            ChannelCommand::Quit => {
                                msg_tx
                                    .send(ChannelMessage::Debug("quitting task".to_string()))
                                    .unwrap();
                                break;
                            }
                        }
                    }
                    if let Err(e) = buf_writer.write_all(line.as_bytes()) {
                        msg_tx
                            .send(ChannelMessage::Error(format!("{:?}", e)))
                            .with_context(|| "error sending ChannelMessage")
                            .unwrap();
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
