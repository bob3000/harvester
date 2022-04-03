use std::{fs::File, io::Write, path::Path, sync::Arc};

use bytes::Bytes;
use futures::{future::join_all, lock::Mutex, Future};
use reqwest::Url;
use tokio::task::JoinHandle;

use crate::{
    filter_list::FilterList,
    input::{url::UrlInput, Input},
};

type SourceDest<SRC, DST> = Vec<(Arc<Mutex<SRC>>, Arc<Mutex<DST>>)>;

/// The FilterController stores the in formation needed to run the data processing
#[derive(Debug)]
pub struct FilterController<'a> {
    lists: &'a [FilterList],
}

impl<'a> FilterController<'a> {
    pub fn new(lists: &'a [FilterList]) -> Self {
        Self { lists }
    }

    /// Runs the data processing function with UrlInput as input source and a
    /// file as output destination
    pub async fn run(&self) -> anyhow::Result<()> {
        let mut src_dest: SourceDest<UrlInput, File> = vec![];
        for list in self.lists.iter() {
            let file = File::create(Path::new(&list.destination))?;
            let url = Url::parse(&list.source)?;
            let input = UrlInput::new(url, None);
            src_dest.push((Arc::new(Mutex::new(input)), Arc::new(Mutex::new(file))));
        }
        let handles = process(src_dest, Arc::new(|chunk| async { Ok(chunk) })).await;
        join_all(handles).await;
        Ok(())
    }
}

/// `process` is the main data processing function. It reads chunks from the source
/// applies a transformation function and writes the data to the output
async fn process<SRC, DST, FN, RES>(
    source_destination: SourceDest<SRC, DST>,
    fn_transform: Arc<FN>,
) -> Vec<JoinHandle<()>>
where
    SRC: Input + Send + Sync + 'static,
    FN: Fn(Bytes) -> RES + Send + Sync + 'static,
    DST: Write + Send + Sync + 'static,
    RES: Future<Output = anyhow::Result<Bytes>> + Send + Sync + 'static,
{
    let mut handles: Vec<JoinHandle<()>> = Vec::new();
    for (input, output) in source_destination {
        let reader = Arc::clone(&input);
        let writer = Arc::clone(&output);
        let fn_trans = Arc::clone(&fn_transform);
        let handle = tokio::spawn(async move {
            while let Some(chunk) = reader.lock().await.chunk().await.unwrap() {
                let chunk = fn_trans(chunk).await.unwrap();
                writer.lock().await.write_all(&chunk[..]).unwrap();
            }
        });
        handles.push(handle);
    }
    handles
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use bytes::Bytes;
    use std::io::{Cursor, Read};

    use super::*;

    #[derive(Debug)]
    struct TestInput {
        cursor: Cursor<String>,
    }

    #[async_trait]
    impl Input for TestInput {
        async fn chunk(&mut self) -> anyhow::Result<Option<Bytes>> {
            let mut buf = vec![0; 32];
            let n = self.cursor.read(&mut buf)?;
            if n == 0 {
                Ok(None)
            } else {
                Ok(Some(Bytes::from(buf)))
            }
        }
    }

    #[tokio::test]
    async fn test_process() {
        let input_data = "line one\nline two\n".to_string();
        let cursor = Cursor::new(input_data.clone());
        let input = Arc::new(Mutex::new(TestInput { cursor }));
        let output = Arc::new(Mutex::new(Cursor::new(vec![0, 32])));
        let handles = process(
            vec![(Arc::clone(&input), Arc::clone(&output))],
            Arc::new(|c| async { Ok(c) }),
        )
        .await;
        join_all(handles).await;
        let o = output.lock().await.clone().into_inner();
        assert!(String::from_utf8_lossy(&o).starts_with(&input_data));
    }
}
