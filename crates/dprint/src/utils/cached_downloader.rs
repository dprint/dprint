use anyhow::Result;
use anyhow::anyhow;
use dprint_core::async_runtime::async_trait;
use std::cell::RefCell;
use std::collections::HashMap;
use url::Url;

use crate::environment::DownloadOptions;
use crate::environment::DownloadedFile;
use crate::environment::UrlDownloader;
use crate::utils::get_bytes_hash;

/// The content and whether it was a 304 Not Modified response.
type CachedDownloadResult = Result<Option<(Vec<u8>, bool)>, String>;
/// The url, a hash of the auth, the proxy, and the cache validators.
type CachedDownloadKey = (String, Option<u64>, String, String);

pub struct CachedDownloader<TInner: UrlDownloader> {
  inner: TInner,
  results: RefCell<HashMap<CachedDownloadKey, CachedDownloadResult>>,
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
  async fn download_file_no_redirects(&self, url: &Url, options: DownloadOptions<'_>) -> Result<Option<DownloadedFile>> {
    let key = (
      url.to_string(),
      options.auth.map(|s| get_bytes_hash(s.as_bytes())),
      format!("{:?}", options.proxy),
      format!("{:?}", options.cache_validators),
    );
    {
      if let Some(result) = self.results.borrow().get(&key) {
        return match result {
          Ok(result) => Ok(result.clone().map(|(content, not_modified)| DownloadedFile {
            headers: Default::default(),
            content,
            not_modified,
          })),
          Err(err) => Err(anyhow!("{:#}", err)),
        };
      }
    }
    let result = self.inner.download_file_no_redirects(url, options).await;
    self.results.borrow_mut().insert(
      key,
      match &result {
        Ok(result) => Ok(result.as_ref().map(|r| (r.content.clone(), r.not_modified))),
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
    let exists_url = Url::parse("http://localhost/test.txt").unwrap();
    let not_exists_url = Url::parse("http://localhost/non-existent.txt").unwrap();
    let environment = builder.add_remote_file(exists_url.as_str(), "1").build();
    environment.clone().run_in_runtime(async move {
      let downloader = CachedDownloader::new(environment.clone());

      // should cache when not exists
      assert!(
        downloader
          .download_file_no_redirects(&not_exists_url, DownloadOptions::default())
          .await
          .as_ref()
          .unwrap()
          .is_none()
      );
      environment.add_remote_file_bytes(not_exists_url.as_str(), Vec::new());
      assert!(
        downloader
          .download_file_no_redirects(&not_exists_url, DownloadOptions::default())
          .await
          .as_ref()
          .unwrap()
          .is_none()
      );

      // should get data and have it cached
      assert_eq!(
        downloader
          .download_file_no_redirects(&exists_url, DownloadOptions::default())
          .await
          .as_ref()
          .unwrap()
          .as_ref()
          .unwrap()
          .content,
        "1".as_bytes()
      );
      environment.add_remote_file_bytes(exists_url.as_str(), Vec::new());
      assert_eq!(
        downloader
          .download_file_no_redirects(&exists_url, DownloadOptions::default())
          .await
          .as_ref()
          .unwrap()
          .as_ref()
          .unwrap()
          .content,
        "1".as_bytes()
      );
    });
  }

  #[test]
  fn should_cache_per_proxy() {
    let mut builder = TestEnvironmentBuilder::new();
    let url = Url::parse("http://localhost/test.txt").unwrap();
    let environment = builder.add_remote_file(url.as_str(), "1").build();
    environment.clone().run_in_runtime(async move {
      let downloader = CachedDownloader::new(environment.clone());
      let with_proxy = DownloadOptions {
        proxy: crate::environment::DownloadProxy::Url("http://proxy:8080"),
        ..Default::default()
      };

      downloader.download_file_no_redirects(&url, with_proxy).await.unwrap().unwrap();
      downloader.download_file_no_redirects(&url, with_proxy).await.unwrap().unwrap();
      assert_eq!(environment.remote_file_request_count(url.as_str()), 1);
      downloader.download_file_no_redirects(&url, DownloadOptions::default()).await.unwrap().unwrap();
      assert_eq!(environment.remote_file_request_count(url.as_str()), 2);
    });
  }

  #[test]
  fn should_cache_per_auth() {
    // entries for the same URL with different auth must not collide:
    // a no-auth lookup mustn't return the auth'd response (or vice versa).
    let mut builder = TestEnvironmentBuilder::new();
    let url = Url::parse("http://localhost/test.txt").unwrap();
    let environment = builder.add_remote_file(url.as_str(), "1").build();
    environment.clone().run_in_runtime(async move {
      let downloader = CachedDownloader::new(environment.clone());
      let bearer = DownloadOptions {
        auth: Some("Bearer T"),
        ..Default::default()
      };

      let with_auth = downloader.download_file_no_redirects(&url, bearer).await.unwrap().unwrap();
      assert_eq!(with_auth.content, "1".as_bytes());

      let no_auth = downloader.download_file_no_redirects(&url, DownloadOptions::default()).await.unwrap().unwrap();
      assert_eq!(no_auth.content, "1".as_bytes());

      // second auth'd call should be served from cache — swap the underlying
      // file and assert the cached body is still returned
      environment.add_remote_file_bytes(url.as_str(), b"changed".to_vec());
      let with_auth_again = downloader.download_file_no_redirects(&url, bearer).await.unwrap().unwrap();
      assert_eq!(with_auth_again.content, "1".as_bytes());
    });
  }
}
