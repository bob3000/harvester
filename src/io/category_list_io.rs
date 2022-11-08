use std::{
    fs::{self, File},
    io::Write,
    path::Path,
    sync::Arc,
};

use anyhow::Context;
use futures::lock::Mutex;

use crate::input::{file::FileInput, Input};

use super::filter_list_io::FilterListIO;

/// CategoryListIO contains a reader and a writer used to manipulate category wise
/// assembled filter lists
#[derive(Debug)]
pub struct CategoryListIO<R: Input + Send, W: Write + Send> {
    pub name: String,
    pub included_filter_lists: Vec<FilterListIO<R, W>>,
    pub reader: Option<Arc<Mutex<R>>>,
    pub writer: Option<Arc<Mutex<W>>>,
}

impl<R: Input + Send, W: Write + Send> CategoryListIO<R, W> {
    /// Create new CategoryListIO with empty reader and writer
    ///
    /// * `name`: the lists name
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            included_filter_lists: vec![],
            reader: None,
            writer: None,
        }
    }
}

impl<W: Write + Send> CategoryListIO<FileInput, W> {
    /// Attaches a potentially existing input file to the reader attribute for inspection
    ///
    /// * `base_dir`: the base directory where the input file is being tried to read
    pub fn attach_existing_input_file(&mut self, base_dir: &Path) -> anyhow::Result<()> {
        let mut contents =
            fs::read_dir(base_dir).with_context(|| "input file directory does not exist")?;
        let entry = contents
            .find(|it| {
                if let Ok(it) = it {
                    return it.file_name().to_str().unwrap() == self.name;
                }
                false
            })
            .ok_or_else(|| anyhow::anyhow!("file not found: {}", self.name))??;
        self.reader = Some(Arc::new(Mutex::new(FileInput::new(entry.path(), None))));
        Ok(())
    }
}

impl CategoryListIO<FileInput, File> {
    /// Tries to read the potential output file for inspection
    ///
    /// * `base_dir`: the base directory where the output file is being tried to read
    pub fn attach_existing_file_writer(&mut self, base_dir: &Path) -> anyhow::Result<()> {
        let out_path = base_dir;
        let mut out_path = out_path.to_path_buf();
        out_path.push(&self.name);
        if !out_path.exists() {
            return Err(anyhow::anyhow!(
                "File {} not found",
                out_path.as_os_str().to_str().unwrap()
            ));
        }
        let out_file =
            File::open(out_path).with_context(|| "could not open out file for reading")?;
        self.writer = Some(Arc::new(Mutex::new(out_file)));
        Ok(())
    }

    /// Creates and output file and it's parent directories, opens the file for writing
    /// and attaches it to the given FilterListIO object
    ///
    /// * `base_dir`: the base directory where the output file is being created
    pub fn attach_new_file_writer(&mut self, base_dir: &Path) -> anyhow::Result<()> {
        let mut out_path = base_dir.to_path_buf();
        fs::create_dir_all(&out_path).with_context(|| "could not create out directory")?;
        out_path.push(&self.name);
        let out_file = File::create(out_path).with_context(|| "could not write out file")?;
        self.writer = Some(Arc::new(Mutex::new(out_file)));
        Ok(())
    }
}
