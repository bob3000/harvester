use std::{
    fs::File,
    pin::Pin,
    sync::{atomic::AtomicBool, Arc},
};

use flume::Sender;
use futures::{lock::Mutex, Future};
use serde::{Deserialize, Serialize};

use crate::{filter_controller::ChannelMessage, input::file::FileInput};

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
        message_tx: Sender<ChannelMessage>,
        is_processing: Arc<AtomicBool>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        match self {
            OutputType::Lua => Box::pin(lua_adapter(reader, writer, message_tx, is_processing)),
            OutputType::Hostsfile => {
                Box::pin(hostsfile_adapter(reader, writer, message_tx, is_processing))
            }
        }
    }
}
