use std::{fs::File, io::Write, sync::Arc};

use anyhow::Context;
use flume::{Receiver, Sender};
use futures::lock::Mutex;

use crate::{
    filter_controller::{ChannelCommand, ChannelMessage},
    input::{file::FileInput, Input},
};

/// lua_adapter translates the extracted URLs int a lua module format
///
/// * `reader`: data source that implements the Input trait
/// * `writer`: data sink that implements std::io::Write
/// * `cmd_rx`: channel listening for commands
/// * `msg_tx`: channel for messaging
pub async fn lua_adapter(
    reader: Arc<Mutex<FileInput>>,
    writer: Arc<Mutex<File>>,
    cmd_rx: Receiver<ChannelCommand>,
    msg_tx: Sender<ChannelMessage>,
) {
    let mut worte_header = false;
    loop {
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
