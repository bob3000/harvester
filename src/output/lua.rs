use std::{
    io::Write,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use anyhow::Context;
use flume::Sender;
use futures::lock::Mutex;

use crate::{filter_controller::ChannelMessage, input::Input};

/// lua_adapter translates the extracted URLs int a lua module format
///
/// * `reader`: data source that implements the Input trait
/// * `writer`: data sink that implements std::io::Write
/// * `cmd_rx`: channel listening for commands
/// * `msg_tx`: channel for messaging
pub async fn lua_adapter(
    reader: Arc<Mutex<dyn Input + Send>>,
    writer: Arc<Mutex<dyn Write + Send>>,
    msg_tx: Sender<ChannelMessage>,
    is_processing: Arc<AtomicBool>,
) {
    let mut worte_header = false;
    loop {
        if !is_processing.load(Ordering::SeqCst) {
            return;
        }
        // write header line
        if !worte_header {
            if let Err(e) = writer.lock().await.write_all("return {\n".as_bytes()) {
                msg_tx
                    .send(ChannelMessage::Error(format!("{}", e)))
                    .with_context(|| "error writing out file")
                    .unwrap();
            }
            worte_header = true;
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
                let chunk = format!("  \"{}\",\n", str_chunk.trim_end());
                if let Err(e) = writer.lock().await.write_all(chunk.as_bytes()) {
                    msg_tx
                        .send(ChannelMessage::Error(format!("{}", e)))
                        .with_context(|| "error writing out file")
                        .unwrap();
                }
            }
            Ok(None) => {
                // write footer line
                if let Err(e) = writer.lock().await.write_all("}".as_bytes()) {
                    msg_tx
                        .send(ChannelMessage::Error(format!("{}", e)))
                        .with_context(|| "error writing out file")
                        .unwrap();
                }
                break;
            }
            Err(e) => {
                msg_tx
                    .send(ChannelMessage::Error(format!("{}", e)))
                    .with_context(|| "error sending ChannelMessage")
                    .unwrap();
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::helper::cursor_input::CursorInput;

    use super::*;
    use flume::Receiver;
    use std::io::Cursor;

    #[tokio::test]
    async fn test_luafile_adapter() {
        // create input data
        let input_data = "domain.one\ndomain.two\n";
        let input = Arc::new(Mutex::new(CursorInput::new(input_data)));
        // set up output sink
        let output = Arc::new(Mutex::new(Cursor::new(vec![0, 32])));
        let (msg_tx, _): (Sender<ChannelMessage>, Receiver<ChannelMessage>) = flume::unbounded();
        let is_processing = Arc::new(AtomicBool::new(true));

        lua_adapter(input, output.clone(), msg_tx, is_processing).await;
        let o = output.lock().await.clone().into_inner();
        let expect = "return {\n  \"domain.one\",\n  \"domain.two\",\n}";
        let got = String::from_utf8_lossy(&o);
        assert_eq!(got, expect);
    }
}
