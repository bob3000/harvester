pub(crate) mod url;

use async_trait::async_trait;
use bytes::Bytes;

/// Input is the trait all input sources must implement
#[async_trait]
pub trait Input {
    /// input sources are supposed to provide the data chunk wise
    async fn chunk(&mut self) -> anyhow::Result<Option<Bytes>>;
}
