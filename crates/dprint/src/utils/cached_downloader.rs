use anyhow::anyhow;
use anyhow::Result;
use dprint_core::async_runtime::async_trait;
use std::cell::RefCell;
use std::collections::HashMap;

use crate::environment::UrlDownloader;

type CachedDownloadResult = Result<Option<Vec<u8>>, String>;

pub struct CachedDownloader<TInner: UrlDownloader> {
  inner: TInner,
  results: RefCell<HashMap<String, CachedDownloadResult>>,
}

impl<TInner: UrlDownloader> CachedDownloader<TInner> {
  pub fn new(inner: TInner) -> Self {
    Self {
      inner,
      results: Default::default(),
    }
  }
}

#[async_trait(?Send)]
impl<TInner: UrlDownloader> UrlDownloader for CachedDownloader<TInner> {
  async fn download_file(&self, url: &str) -> Result<Option<Vec<u8>>> {
    {
      if let Some(result) = self.results.borrow().get(url) {
        return match result {
          Ok(result) => Ok(result.clone()),
          Err(err) => Err(anyhow!("{:#}", err)),
        };
      }
    }
    let result = self.inner.download_file(url).await;
    self.results.borrow_mut().insert(
      url.to_string(),
      match &result {
        Ok(result) => Ok(result.clone()),
        Err(err) => Err(format!("{:#}", err)),
      },
    );
    result
  }
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::environment::TestEnvironmentBuilder;

  #[test]
  fn should_download_and_cache() {
    let mut builder = TestEnvironmentBuilder::new();
    let exists_url = "http://localhost/test.txt";
    let not_exists_url = "http://localhost/non-existent.txt";
    let environment = builder.add_remote_file(exists_url, "1").build();
    environment.clone().run_in_runtime(async move {
      let downloader = CachedDownloader::new(environment.clone());

      // should cache when not exists
      assert!(downloader.download_file(not_exists_url).await.as_ref().unwrap().is_none());
      environment.add_remote_file_bytes(not_exists_url, Vec::new());
      assert!(downloader.download_file(not_exists_url).await.as_ref().unwrap().is_none());

      // should get data and have it cached
      assert_eq!(downloader.download_file(exists_url).await.as_ref().unwrap().as_ref().unwrap(), "1".as_bytes());
      environment.add_remote_file_bytes(exists_url, Vec::new());
      assert_eq!(downloader.download_file(exists_url).await.as_ref().unwrap().as_ref().unwrap(), "1".as_bytes());
    });
  }
}
