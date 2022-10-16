use async_trait::async_trait;
use std::io::{BufRead, BufReader, Cursor};

use crate::input::Input;

/// TestInput implements the Input trait using Cursors
#[derive(Debug)]
pub struct CursorInput {
    input_data: String,
    cursor: BufReader<Cursor<String>>,
}

impl CursorInput {
    pub fn new(input_data: &str) -> Self {
        let cursor = Cursor::new(input_data.to_owned().clone());
        let buf_cursor = BufReader::new(cursor);
        CursorInput {
            input_data: input_data.to_string(),
            cursor: buf_cursor,
        }
    }
}

#[async_trait]
impl Input for CursorInput {
    async fn chunk(&mut self) -> anyhow::Result<Option<Vec<u8>>> {
        let mut line = String::new();
        let n = self.cursor.read_line(&mut line)?;
        if n == 0 {
            Ok(None)
        } else {
            Ok(Some(line.as_bytes().to_vec()))
        }
    }

    async fn reset(&mut self) -> anyhow::Result<()> {
        self.cursor = BufReader::new(Cursor::new(self.input_data.clone()));
        Ok(())
    }
}
