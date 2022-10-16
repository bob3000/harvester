use std::{
    io::Write,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use futures::lock::Mutex;

use crate::input::Input;

/// hostsfile_adapter translates the extracted URLs int a hosts file format
/// as found in /etc/hosts
///
/// * `reader`: data source that implements the Input trait
/// * `writer`: data sink that implements std::io::Write
/// * `cmd_rx`: channel listening for commands
/// * `msg_tx`: channel for messaging
pub async fn hostsfile_adapter(
    reader: Arc<Mutex<dyn Input + Send>>,
    writer: Arc<Mutex<dyn Write + Send>>,
    is_processing: Arc<AtomicBool>,
) {
    loop {
        if !is_processing.load(Ordering::SeqCst) {
            return;
        }
        match reader.lock().await.chunk().await {
            Ok(Some(chunk)) => {
                let str_chunk = match String::from_utf8(chunk) {
                    Ok(s) => s,
                    Err(e) => {
                        anyhow::anyhow!("{}", e);
                        continue;
                    }
                };
                let chunk = format!("0.0.0.0 {}\n", str_chunk.trim_end());
                if let Err(e) = writer.lock().await.write_all(chunk.as_bytes()) {
                    error!("{}", e);
                }
            }
            Ok(None) => {
                break;
            }
            Err(e) => {
                error!("{}", e);
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::helper::cursor_input::CursorInput;

    use super::*;
    use std::io::Cursor;

    #[tokio::test]
    async fn test_hostfile_adapter() {
        // create input data
        let input_data = "domain.one\ndomain.two\n";
        let input = Arc::new(Mutex::new(CursorInput::new(input_data)));
        // set up output sink
        let output = Arc::new(Mutex::new(Cursor::new(vec![0, 32])));
        let is_processing = Arc::new(AtomicBool::new(true));

        hostsfile_adapter(input, output.clone(), is_processing).await;
        let o = output.lock().await.clone().into_inner();
        let expect = "0.0.0.0 domain.one\n0.0.0.0 domain.two\n";
        let got = String::from_utf8_lossy(&o);
        assert_eq!(got, expect);
    }
}
