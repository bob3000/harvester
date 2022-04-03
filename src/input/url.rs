use crate::input::Input;
use async_trait::async_trait;
use bytes::Bytes;
use reqwest::Url;

/// UrlInput downloads data from an Url
#[derive(Debug)]
pub(crate) struct UrlInput {
    url: Url,
    response: Option<reqwest::Response>,
}

impl UrlInput {
    pub fn new(url: Url, response: Option<reqwest::Response>) -> Self {
        Self { url, response }
    }
}

#[async_trait]
impl Input for UrlInput {
    async fn chunk(&mut self) -> anyhow::Result<Option<Bytes>> {
        if self.response.is_none() {
            self.response = Some(reqwest::get(self.url.clone()).await?);
        }
        match self.response.as_mut().unwrap().chunk().await {
            Ok(r) => Ok(r),
            Err(e) => Err(anyhow::anyhow!(e)),
        }
    }
}
