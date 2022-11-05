use anyhow::Context;
use std::{
    fs::{self, File},
    io::Write,
    path::PathBuf,
};

use crate::config::Config;

pub const TEST_CACHE: &str = "test_cache";
pub const TEST_OUT: &str = "test_out";

#[derive(Debug)]
pub struct CacheFileCreator {
    pub inpath: String,
    pub outpath: String,
}

impl CacheFileCreator {
    pub fn new(inpath: &str, outpath: &str) -> Self {
        let mut test_inpath = PathBuf::from(TEST_CACHE);
        test_inpath.push(inpath);
        let mut test_outpath = PathBuf::from(TEST_CACHE);
        test_outpath.push(outpath);
        fs::create_dir_all(&test_inpath)
            .with_context(|| "inpath")
            .ok();
        fs::create_dir_all(&test_outpath)
            .with_context(|| "outpath")
            .ok();
        fs::remove_file(outpath).ok();
        Self {
            inpath: inpath.to_string(),
            outpath: outpath.to_string(),
        }
    }

    pub fn write_input(&self, list_id: &str, input: &str) {
        let mut infile_path = PathBuf::from(TEST_CACHE);
        infile_path.push(&self.inpath);
        infile_path.push(list_id);
        let mut infile = File::create(infile_path)
            .with_context(|| "infile error")
            .unwrap();
        infile.write_all(input.as_bytes()).unwrap();
    }

    pub fn new_test_config(&self) -> Config {
        Config {
            lists: vec![],
            cache_dir: TEST_CACHE.to_string(),
            output_dir: TEST_OUT.to_string(),
            output_format: crate::output::OutputType::Hostsfile,
            cached_config: None,
        }
    }

    pub fn read_result(&self, list_id: &str) -> anyhow::Result<String> {
        let mut outfile_path = PathBuf::from(TEST_CACHE);
        outfile_path.push(&self.outpath);
        outfile_path.push(list_id);
        let result = fs::read_to_string(&outfile_path)
            .with_context(|| format!("{} not found", &outfile_path.to_str().unwrap()));
        result
    }
}
