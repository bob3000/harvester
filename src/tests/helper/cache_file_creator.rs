use anyhow::Context;
use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};

use crate::config::Config;

pub const TEST_CACHE: &str = "test_cache";

#[derive(Debug)]
pub struct CacheFileCreator {
    pub namespace: String,
    pub inpath: String,
    pub outpath: String,
}

fn namespace_path(namespace: impl AsRef<Path>, dir: Option<impl AsRef<Path>>) -> PathBuf {
    let mut target_path = PathBuf::from(TEST_CACHE);
    target_path.push(namespace);
    if dir.is_some() {
        target_path.push(dir.unwrap());
    }
    target_path
}

impl CacheFileCreator {
    pub fn new(namespace: &str, inpath: &str, outpath: &str) -> Self {
        let test_inpath = namespace_path(namespace, Some(inpath));
        let test_outpath = namespace_path(namespace, Some(outpath));
        fs::create_dir_all(&test_inpath)
            .with_context(|| "inpath")
            .ok();
        fs::create_dir_all(&test_outpath)
            .with_context(|| "outpath")
            .ok();
        fs::remove_file(outpath).ok();
        Self {
            namespace: namespace.to_string(),
            inpath: inpath.to_string(),
            outpath: outpath.to_string(),
        }
    }

    pub fn write_input(&self, list_id: &str, input: &str) {
        let mut infile_path = namespace_path(&self.namespace, Some(&self.inpath));
        infile_path.push(list_id);
        let mut infile = File::create(infile_path)
            .with_context(|| "infile error")
            .unwrap();
        infile.write_all(input.as_bytes()).unwrap();
    }

    pub fn new_test_config(&self) -> Config {
        Config {
            lists: vec![],
            cache_dir: namespace_path(&self.namespace, None::<&str>)
                .to_str()
                .unwrap()
                .to_string(),
            output_dir: namespace_path(&self.namespace, Some("output"))
                .to_str()
                .unwrap()
                .to_string(),
            output_format: crate::output::OutputType::Hostsfile,
            cached_config: None,
        }
    }

    pub fn read_result(&self, list_id: &str) -> anyhow::Result<String> {
        let mut outfile_path = namespace_path(&self.namespace, Some(&self.outpath));
        outfile_path.push(list_id);
        let result = fs::read_to_string(&outfile_path)
            .with_context(|| format!("{} not found", &outfile_path.to_str().unwrap()));
        result
    }
}
