use std::{fs::File, pin::Pin, sync::Arc};

use flume::{Receiver, Sender};
use futures::{lock::Mutex, Future};
use serde::{Deserialize, Serialize};

use crate::{
    filter_controller::{ChannelCommand, ChannelMessage},
    input::file::FileInput,
};

use self::{hostsfile::hostsfile_adapter, lua::lua_adapter};

mod hostsfile;
mod lua;

/// OutputType represents a result format for the created block lists
#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum OutputType {
    /// Lua module format
    Lua,
    /// Hostsfile format as found in /etc/hosts
    Hostsfile,
}

impl OutputType {
    pub fn get_adapter<'a>(
        &self,
        reader: Arc<Mutex<FileInput>>,
        writer: Arc<Mutex<File>>,
        command_rx: Receiver<ChannelCommand>,
        message_tx: Sender<ChannelMessage>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        match self {
            OutputType::Lua => Box::pin(lua_adapter(reader, writer, command_rx, message_tx)),
            OutputType::Hostsfile => {
                Box::pin(hostsfile_adapter(reader, writer, command_rx, message_tx))
            }
        }
    }
}
