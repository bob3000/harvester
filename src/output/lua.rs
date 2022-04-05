use std::{fs::File, io::Write, sync::Arc};

use anyhow::Context;
use flume::{Receiver, Sender};
use futures::lock::Mutex;

use crate::{
    filter_controller::{ChannelCommand, ChannelMessage},
    input::{file::FileInput, Input},
};

pub async fn lua_adapter(
    reader: Arc<Mutex<FileInput>>,
    writer: Arc<Mutex<File>>,
    cmd_rx: Receiver<ChannelCommand>,
    msg_tx: Sender<ChannelMessage>,
) {
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
        // let r = reader.lock().await.chunk().await;
        match reader.lock().await.chunk().await {
            Ok(Some(chunk)) => {
                if let Err(e) = writer.lock().await.write_all(chunk.as_bytes()) {
                    msg_tx
                        .send(ChannelMessage::Error(format!("{}", e)))
                        .with_context(|| "error writing out file")
                        .unwrap();
                }
            }
            Ok(None) => break,
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

