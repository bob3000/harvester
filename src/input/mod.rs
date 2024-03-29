pub(crate) mod file;
pub(crate) mod url;

use async_trait::async_trait;

/// Input is the trait all input sources must implement
#[async_trait]
pub trait Input {
    /// input sources are supposed to provide the data chunk wise
    async fn chunk(&mut self) -> anyhow::Result<Option<Vec<u8>>>;

    /// start reading from the beginning
    async fn reset(&mut self) -> anyhow::Result<()>;

    /// returns the length of the content if available
    async fn len(&mut self) -> anyhow::Result<u64>;
}
