use crate::input::Input;
use async_trait::async_trait;
use reqwest::Url;

/// UrlInput downloads data from an Url
#[derive(Debug)]
pub struct UrlInput {
    url: Url,
    response: Option<reqwest::Response>,
}

impl UrlInput {
    pub fn new(url: Url) -> Self {
        Self {
            url,
            response: None,
        }
    }
}

#[async_trait]
impl Input for UrlInput {
    async fn chunk(&mut self) -> anyhow::Result<Option<String>> {
        if self.response.is_none() {
            self.response = Some(reqwest::get(self.url.clone()).await?);
        }
        match self.response.as_mut().unwrap().chunk().await {
            Ok(Some(r)) => {
                let r = String::from_utf8(r.to_vec())?;
                Ok(Some(r))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(anyhow::anyhow!(e)),
        }
    }
}
