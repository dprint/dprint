use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use deno_terminal::colors;
use url::Url;

use super::PathSource;
use crate::cache::HttpCache;
use crate::cache::HttpCacheSys;
use crate::cache::RemoteCacheMode;
use crate::environment::CacheValidators;
use crate::environment::DownloadOptions;
use crate::environment::Environment;
use crate::utils::RemotePathSource;

const MAX_REDIRECTS: usize = 10;

#[derive(Debug, Clone)]
pub struct ResolvedFilePathWithBytes {
  pub source: PathSource,
  pub is_first_download: bool,
  pub content: Vec<u8>,
}

impl ResolvedFilePathWithBytes {
  pub fn into_text(self) -> Result<ResolvedFilePathWithText> {
    let content = String::from_utf8(self.content).with_context(|| format!("Failed converting '{}' to string.", self.source.display()))?;
    Ok(ResolvedFilePathWithText {
      source: self.source,
      content,
      is_first_download: self.is_first_download,
    })
  }
}

#[derive(Debug, Clone)]
pub struct ResolvedFilePathWithText {
  pub source: PathSource,
  pub is_first_download: bool,
  pub content: String,
}

impl ResolvedFilePathWithText {
  pub fn as_ref(&self) -> ResolvedFilePathWithTextRef<'_> {
    ResolvedFilePathWithTextRef {
      source: &self.source,
      content: &self.content,
    }
  }
}

#[derive(Debug, Clone, Copy)]
pub struct ResolvedFilePathWithTextRef<'a> {
  pub source: &'a PathSource,
  pub content: &'a str,
}

pub async fn resolve_url_or_file_path_to_file_with_cache<TEnvironment: Environment>(
  url_or_file_path: &str,
  base: &PathSource,
  cache_mode: RemoteCacheMode,
  environment: &TEnvironment,
) -> Result<ResolvedFilePathWithBytes> {
  let path_source = resolve_url_or_file_path_to_path_source(url_or_file_path, base, environment)?;

  match &path_source {
    PathSource::Remote(remote_path_source) => resolve_url_to_file_with_cache(&remote_path_source.url, cache_mode, environment).await,
    PathSource::Local(local_path_source) => {
      let content = environment.read_file_bytes(&local_path_source.path)?;
      Ok(ResolvedFilePathWithBytes {
        source: path_source,
        is_first_download: false,
        content,
      })
    }
    PathSource::Npm(_) => bail!("Cannot resolve npm specifier as a URL or file path"),
  }
}

/// Resolves the url using the cached response while it's fresh (see
/// `SerializedCachedUrlMetadata::is_fresh_at`). Once it's stale or when
/// reloading, the server is asked for a newer response, which it can answer
/// with 304 Not Modified when the cached one is still current. A stale
/// cached response is used when the server can't be reached.
async fn resolve_url_to_file_with_cache<TEnvironment: Environment>(
  url: &Url,
  cache_mode: RemoteCacheMode,
  environment: &TEnvironment,
) -> Result<ResolvedFilePathWithBytes> {
  let cache = HttpCache::new(environment.clone(), environment.get_cache_dir().join("remote"));

  if cache_mode == RemoteCacheMode::Use
    && let Some(cached) = read_cached_chain(&cache, url, /* allow stale */ false)?
  {
    return Ok(cached);
  }

  match download_through_cache(&cache, url, cache_mode, environment).await {
    Ok(Some(result)) => Ok(result),
    Ok(None) => bail!("Error downloading {} - 404 Not Found", url),
    Err(err) => match read_cached_chain(&cache, url, /* allow stale */ true) {
      Ok(Some(cached)) => {
        log_warn!(
          environment,
          "{} Failed checking for a newer version of {}, so using the cached one.\n    {:#}",
          colors::yellow("Warning"),
          url,
          err
        );
        Ok(cached)
      }
      // the download error is the useful one
      Ok(None) | Err(_) => Err(err),
    },
  }
}

/// Reads the cached response of the url, following cached redirects. Returns
/// `None` when any response in the chain isn't cached or, unless stale ones
/// are allowed, is stale.
///
/// The whole chain is checked because a redirect target might only be valid
/// for a short time (ex. a signed url), so it should be requested again via
/// the original url rather than directly.
fn read_cached_chain<Sys: HttpCacheSys>(cache: &HttpCache<Sys>, url: &Url, allow_stale: bool) -> Result<Option<ResolvedFilePathWithBytes>> {
  let mut current_url = url.clone();
  for _ in 0..=MAX_REDIRECTS {
    let key = cache.cache_item_key(&current_url)?;
    let Some(entry) = cache.get(&key)? else {
      return Ok(None);
    };
    if !allow_stale && !cache.is_fresh(&entry.metadata) {
      return Ok(None);
    }
    if let Some(location) = entry.metadata.headers.get("location") {
      current_url = current_url.join(location)?;
      continue;
    }
    return Ok(Some(ResolvedFilePathWithBytes {
      source: PathSource::Remote(RemotePathSource { url: current_url }),
      is_first_download: false,
      content: entry.content,
    }));
  }
  bail!("Too many redirects for {}", url)
}

/// Downloads the url, following redirects and caching every response along
/// the way. The validators of a cached response are sent so the server can
/// respond with 304 Not Modified, in which case the cached content is used.
/// Returns `None` on a 404.
async fn download_through_cache<TEnvironment: Environment>(
  cache: &HttpCache<TEnvironment>,
  url: &Url,
  cache_mode: RemoteCacheMode,
  environment: &TEnvironment,
) -> Result<Option<ResolvedFilePathWithBytes>> {
  let mut current_url = url.clone();
  for i in 0..=MAX_REDIRECTS {
    // a redirect may lead back to a fresh part of the chain
    if i > 0
      && cache_mode == RemoteCacheMode::Use
      && let Some(cached) = read_cached_chain(cache, &current_url, /* allow stale */ false)?
    {
      return Ok(Some(cached));
    }

    let key = cache.cache_item_key(&current_url)?;
    let cached = cache.get(&key)?;
    let cache_validators = match &cached {
      // the validators of a cached redirect aren't useful
      Some(entry) if !entry.metadata.headers.contains_key("location") => CacheValidators {
        etag: entry.metadata.headers.get("etag").map(|s| s.as_str()),
        last_modified: entry.metadata.headers.get("last-modified").map(|s| s.as_str()),
      },
      _ => CacheValidators::default(),
    };

    let Some(result) = environment
      .download_file_no_redirects(
        &current_url,
        DownloadOptions {
          cache_validators,
          ..Default::default()
        },
      )
      .await?
    else {
      return Ok(None);
    };

    if result.not_modified {
      let Some(entry) = cached else {
        bail!("Error downloading {} - 304 Not Modified without a cached response", current_url);
      };
      // the cached response stays current with the headers of the 304 response,
      // and the ones about when the original response was received don't apply
      // anymore
      let mut headers = entry.metadata.headers;
      headers.remove("age");
      headers.remove("date");
      headers.extend(result.headers);
      // cache the response and ignore errors
      _ = cache.set(&current_url, headers, &entry.content);
      return Ok(Some(ResolvedFilePathWithBytes {
        source: PathSource::Remote(RemotePathSource { url: current_url }),
        is_first_download: false,
        content: entry.content,
      }));
    }

    // cache the response and ignore errors
    _ = cache.set(&current_url, result.headers.clone(), &result.content);

    // follow redirect
    if let Some(location) = result.headers.get("location") {
      current_url = current_url.join(location)?;
      continue;
    }

    return Ok(Some(ResolvedFilePathWithBytes {
      source: PathSource::Remote(RemotePathSource { url: current_url }),
      is_first_download: true,
      content: result.content,
    }));
  }

  bail!("Too many redirects for {}", url)
}

pub async fn fetch_file_or_url_bytes(url_or_file_path: &PathSource, environment: &impl Environment) -> Result<Vec<u8>> {
  match url_or_file_path {
    PathSource::Remote(path_source) => Ok(environment.download_file_err_404(&path_source.url, DownloadOptions::default()).await?.1.content),
    PathSource::Local(path_source) => Ok(environment.read_file_bytes(&path_source.path)?),
    PathSource::Npm(_) => bail!("Cannot fetch bytes directly for an npm specifier"),
  }
}

pub fn resolve_url_or_file_path_to_path_source(url_or_file_path: &str, base: &PathSource, environment: &impl Environment) -> Result<PathSource> {
  if let Some(url) = try_parse_url(url_or_file_path) {
    if url.cannot_be_a_base() {
      // relative url
      if let PathSource::Remote(remote_base) = base {
        let url = remote_base.url.join(url_or_file_path)?;
        return Ok(PathSource::new_remote(url));
      }
    } else {
      // handle file urls (ex. file:///C:/some/folder/file.json)
      if url.scheme() == "file" {
        match url.to_file_path() {
          Ok(file_path) => return Ok(PathSource::new_local(environment.canonicalize(file_path)?)),
          Err(()) => bail!("Problem converting file url `{}` to file path.", url_or_file_path),
        }
      }
      return Ok(PathSource::new_remote(url));
    }
  } else if let Some(rest) = url_or_file_path.strip_prefix("~/") {
    // handle home directory
    match environment.get_home_dir() {
      Some(home_dir) => {
        let path = if rest.is_empty() {
          home_dir
        } else {
          environment.canonicalize(home_dir.join(rest))?
        };
        return Ok(PathSource::new_local(path));
      }
      None => bail!("Failed to get home directory path"),
    }
  }

  Ok(match base {
    PathSource::Remote(remote_base) => {
      let url = remote_base.url.join(url_or_file_path)?;
      PathSource::new_remote(url)
    }
    PathSource::Local(local_base) => PathSource::new_local(environment.canonicalize(local_base.path.join(url_or_file_path))?),
    PathSource::Npm(_) => bail!("Cannot resolve a relative path against an npm specifier"),
  })
}

fn try_parse_url(url_or_file_path: &str) -> Option<Url> {
  if is_absolute_windows_file_path(url_or_file_path) {
    return None;
  }

  Url::parse(url_or_file_path).ok()
}

fn is_absolute_windows_file_path(value: &str) -> bool {
  let chars = value.chars().collect::<Vec<_>>();
  return is_alpha(&chars, 0) && matches!(chars.get(1), Some(':')) && is_slash(&chars, 2) && !is_slash(&chars, 3);

  fn is_alpha(chars: &[char], index: usize) -> bool {
    chars.get(index).map(|c| c.is_alphabetic()).unwrap_or(false)
  }

  fn is_slash(chars: &[char], index: usize) -> bool {
    chars.get(index).map(|c| matches!(c, '/' | '\\')).unwrap_or(false)
  }
}

#[cfg(test)]
mod tests {
  use std::path::Path;

  use crate::environment::CanonicalizedPathBuf;
  use crate::environment::TestEnvironment;
  use pretty_assertions::assert_eq;

  use super::super::PathSource;
  use super::*;

  #[test]
  fn should_resolve_a_url() {
    let environment = TestEnvironment::new();
    environment.add_remote_file("https://dprint.dev/test.json", "t".as_bytes());
    environment.clone().run_in_runtime(async move {
      let base = PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/"));
      let url = "https://dprint.dev/test.json";
      let result = resolve_url_or_file_path_to_file_with_cache(url, &base, RemoteCacheMode::Use, &environment)
        .await
        .unwrap();
      assert_eq!(result.source.is_remote(), true);
      assert_eq!(result.is_first_download, true);
      assert_eq!(result.content, "t".as_bytes());

      // should get a second time from the cache
      let result = resolve_url_or_file_path_to_file_with_cache(url, &base, RemoteCacheMode::Use, &environment)
        .await
        .unwrap();
      assert_eq!(result.source.is_remote(), true);
      assert_eq!(result.is_first_download, false);
      assert_eq!(result.content, "t".as_bytes());
    });
  }

  #[test]
  fn should_resolve_a_relative_path_to_base_url() {
    let environment = TestEnvironment::new();
    environment.add_remote_file("https://dprint.dev/asdf/test/test.json", "t".as_bytes());
    environment.clone().run_in_runtime(async move {
      let base = PathSource::new_remote(Url::parse("https://dprint.dev/asdf/").unwrap());
      let result = resolve_url_or_file_path_to_file_with_cache("test/test.json", &base, RemoteCacheMode::Use, &environment)
        .await
        .unwrap();
      assert_eq!(result.source.is_remote(), true);
      assert_eq!(result.source.unwrap_remote().url.as_str(), "https://dprint.dev/asdf/test/test.json");
      assert_eq!(result.content, "t".as_bytes());
    });
  }

  #[cfg(windows)]
  #[test]
  fn should_resolve_a_file_url_on_windows() {
    let environment = TestEnvironment::new();
    environment.mk_dir_all("C:\\test").unwrap();
    environment.write_file("C:\\test\\test.json", "{}").unwrap();
    environment.clone().run_in_runtime(async move {
      use crate::environment::CanonicalizedPathBuf;

      let base = PathSource::new_local(CanonicalizedPathBuf::new_for_testing("V:\\"));
      let result = resolve_url_or_file_path_to_file_with_cache("file://C:/test/test.json", &base, RemoteCacheMode::Use, &environment)
        .await
        .unwrap();
      assert_eq!(result.source.is_local(), true);
      assert_eq!(result.source.unwrap_local().path, CanonicalizedPathBuf::new_for_testing("C:\\test\\test.json"));
    });
  }

  #[cfg(unix)]
  #[test]
  fn should_resolve_a_file_url_on_unix() {
    let environment = TestEnvironment::new();
    environment.mk_dir_all("/test").unwrap();
    environment.write_file("/test/test.json", "{}").unwrap();
    environment.clone().run_in_runtime(async move {
      let base = PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/"));
      let result = resolve_url_or_file_path_to_file_with_cache("file:///test/test.json", &base, RemoteCacheMode::Use, &environment)
        .await
        .unwrap();
      assert_eq!(result.source.is_local(), true);
      assert_eq!(result.source.unwrap_local().path, CanonicalizedPathBuf::new_for_testing("/test/test.json"));
    });
  }

  #[cfg(windows)]
  #[test]
  fn should_resolve_an_absolute_path_on_windows() {
    let environment = TestEnvironment::new();
    environment.mk_dir_all("C:\\test").unwrap();
    environment.write_file("C:\\test\\test.json", "{}").unwrap();
    environment.clone().run_in_runtime(async move {
      let base = PathSource::new_local(CanonicalizedPathBuf::new_for_testing("V:\\"));
      let result = resolve_url_or_file_path_to_file_with_cache("C:\\test\\test.json", &base, RemoteCacheMode::Use, &environment)
        .await
        .unwrap();
      assert_eq!(result.source.is_local(), true);
      assert_eq!(result.source.unwrap_local().path, CanonicalizedPathBuf::new_for_testing("C:\\test\\test.json"));
    });
  }

  #[cfg(windows)]
  #[test]
  fn should_resolve_an_absolute_path_on_windows_using_forward_slashes() {
    let environment = TestEnvironment::new();
    environment.mk_dir_all("C:\\test").unwrap();
    environment.write_file("C:\\test\\test.json", "{}").unwrap();
    environment.clone().run_in_runtime(async move {
      let base = PathSource::new_local(CanonicalizedPathBuf::new_for_testing("V:\\"));
      let result = resolve_url_or_file_path_to_file_with_cache("C:/test/test.json", &base, RemoteCacheMode::Use, &environment)
        .await
        .unwrap();
      assert_eq!(result.source.is_local(), true);
      assert_eq!(result.source.unwrap_local().path, CanonicalizedPathBuf::new_for_testing("C:\\test\\test.json"));
    });
  }

  #[test]
  fn should_resolve_a_relative_file_path() {
    let environment = TestEnvironment::new();
    environment.mk_dir_all("/test").unwrap();
    environment.write_file("/test/test.json", "{}").unwrap();
    environment.clone().run_in_runtime(async move {
      let base = PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/"));
      let result = resolve_url_or_file_path_to_file_with_cache("test/test.json", &base, RemoteCacheMode::Use, &environment)
        .await
        .unwrap();
      assert_eq!(result.source.is_local(), true);
      assert_eq!(result.source.unwrap_local().path, CanonicalizedPathBuf::new_for_testing("/test/test.json"));
    });
  }

  #[test]
  fn should_resolve_a_file_path_relative_to_base_path() {
    let environment = TestEnvironment::new();
    environment.mk_dir_all("/other/test").unwrap();
    environment.write_file("/other/test/test.json", "{}").unwrap();
    environment.clone().run_in_runtime(async move {
      let base = PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/other"));
      let result = resolve_url_or_file_path_to_file_with_cache("test/test.json", &base, RemoteCacheMode::Use, &environment)
        .await
        .unwrap();
      assert_eq!(result.source.is_local(), true);
      assert_eq!(
        result.source.unwrap_local().path,
        CanonicalizedPathBuf::new_for_testing("/other/test/test.json")
      );
    });
  }

  #[test]
  fn should_error_when_url_cannot_be_resolved() {
    let environment = TestEnvironment::new();
    environment.clone().run_in_runtime(async move {
      let base = PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/other"));
      let err = resolve_url_or_file_path_to_file_with_cache("https://dprint.dev/test.json", &base, RemoteCacheMode::Use, &environment)
        .await
        .err()
        .unwrap();
      assert_eq!(err.to_string(), "Error downloading https://dprint.dev/test.json - 404 Not Found");
    });
  }

  #[test]
  fn should_resolve_url_using_redirected_url() {
    let environment = TestEnvironment::new();
    environment.add_remote_file("https://cdn.example.com/v1/plugin.json", "content".as_bytes());
    environment.add_remote_file_redirect("https://example.com/plugin.json", "https://cdn.example.com/v1/plugin.json");
    environment.clone().run_in_runtime(async move {
      let base = PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/"));
      let result = resolve_url_or_file_path_to_file_with_cache("https://example.com/plugin.json", &base, RemoteCacheMode::Use, &environment)
        .await
        .unwrap();
      assert_eq!(result.source.is_remote(), true);
      assert_eq!(result.is_first_download, true);
      assert_eq!(result.content, "content".as_bytes());
      // the resolved path source should use the redirected URL
      assert_eq!(
        result.source,
        PathSource::new_remote(Url::parse("https://cdn.example.com/v1/plugin.json").unwrap())
      );
      // relative paths should resolve against the redirected URL
      let relative_result = resolve_url_or_file_path_to_path_source("downloads/plugin.zip", &result.source.parent(), &environment).unwrap();
      assert_eq!(
        relative_result,
        PathSource::new_remote(Url::parse("https://cdn.example.com/v1/downloads/plugin.zip").unwrap())
      );

      // should get from cache on second request and still have correct redirect URL
      let result2 = resolve_url_or_file_path_to_file_with_cache("https://example.com/plugin.json", &base, RemoteCacheMode::Use, &environment)
        .await
        .unwrap();
      assert_eq!(result2.is_first_download, false);
      assert_eq!(
        result2.source,
        PathSource::new_remote(Url::parse("https://cdn.example.com/v1/plugin.json").unwrap())
      );
    });
  }

  #[test]
  fn should_use_cached_response_while_fresh_then_check_server() {
    let environment = TestEnvironment::new();
    let url = "https://dprint.dev/config.json";
    environment.set_fs_time(1_000);
    environment.add_remote_file_with_headers(url, b"v1", &[("etag", "\"abc\""), ("cache-control", "max-age=300")]);
    environment.clone().run_in_runtime(async move {
      let base = PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/"));
      let result = resolve_url_or_file_path_to_file_with_cache(url, &base, RemoteCacheMode::Use, &environment)
        .await
        .unwrap();
      assert_eq!(result.is_first_download, true);
      assert_eq!(result.content, b"v1");
      assert_eq!(
        environment.take_remote_file_cache_validators(url).unwrap(),
        "CacheValidators { etag: None, last_modified: None }"
      );

      // fresh, so the server isn't asked
      environment.set_fs_time(1_299);
      let result = resolve_url_or_file_path_to_file_with_cache(url, &base, RemoteCacheMode::Use, &environment)
        .await
        .unwrap();
      assert_eq!(result.is_first_download, false);
      assert_eq!(result.content, b"v1");
      assert_eq!(environment.remote_file_request_count(url), 1);

      // stale, so the server is asked with the validators and says it's unchanged
      environment.set_fs_time(1_300);
      let result = resolve_url_or_file_path_to_file_with_cache(url, &base, RemoteCacheMode::Use, &environment)
        .await
        .unwrap();
      assert_eq!(result.is_first_download, false);
      assert_eq!(result.content, b"v1");
      assert_eq!(environment.remote_file_request_count(url), 2);
      assert_eq!(
        environment.take_remote_file_cache_validators(url).unwrap(),
        "CacheValidators { etag: Some(\"\\\"abc\\\"\"), last_modified: None }"
      );

      // the unchanged response is fresh again
      environment.set_fs_time(1_599);
      resolve_url_or_file_path_to_file_with_cache(url, &base, RemoteCacheMode::Use, &environment)
        .await
        .unwrap();
      assert_eq!(environment.remote_file_request_count(url), 2);

      // then the file changes
      environment.set_fs_time(1_600);
      environment.add_remote_file_with_headers(url, b"v2", &[("etag", "\"def\""), ("cache-control", "max-age=300")]);
      let result = resolve_url_or_file_path_to_file_with_cache(url, &base, RemoteCacheMode::Use, &environment)
        .await
        .unwrap();
      assert_eq!(result.is_first_download, true);
      assert_eq!(result.content, b"v2");
      assert_eq!(environment.remote_file_request_count(url), 3);
      assert!(environment.take_stderr_messages().is_empty());
    });
  }

  #[test]
  fn should_check_server_for_response_cached_without_time_by_older_version() {
    let environment = TestEnvironment::new();
    let url = "https://dprint.dev/config.json";
    environment.set_fs_time(1_000);
    environment.add_remote_file_with_headers(url, b"v2", &[("etag", "\"v2\"")]);
    environment.clone().run_in_runtime(async move {
      let cache = HttpCache::new(environment.clone(), environment.get_cache_dir().join("remote"));
      let parsed_url = Url::parse(url).unwrap();
      cache
        .set_for_testing(
          &parsed_url,
          b"v1",
          crate::cache::SerializedCachedUrlMetadata {
            headers: [("etag".to_string(), "\"v1\"".to_string())].into_iter().collect(),
            url: url.to_string(),
            time: None,
          },
        )
        .unwrap();

      let base = PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/"));
      let result = resolve_url_or_file_path_to_file_with_cache(url, &base, RemoteCacheMode::Use, &environment)
        .await
        .unwrap();
      assert_eq!(result.content, b"v2");
      assert_eq!(result.is_first_download, true);
      assert_eq!(
        environment.take_remote_file_cache_validators(url).unwrap(),
        "CacheValidators { etag: Some(\"\\\"v1\\\"\"), last_modified: None }"
      );

      // now it has a time
      resolve_url_or_file_path_to_file_with_cache(url, &base, RemoteCacheMode::Use, &environment)
        .await
        .unwrap();
      assert_eq!(environment.remote_file_request_count(url), 1);
    });
  }

  #[test]
  fn should_not_keep_age_of_original_response_after_not_modified() {
    let environment = TestEnvironment::new();
    let url = "https://dprint.dev/config.json";
    environment.set_fs_time(1_000);
    environment.add_remote_file_with_headers(url, b"v1", &[("etag", "\"abc\""), ("cache-control", "max-age=300"), ("age", "250")]);
    environment.clone().run_in_runtime(async move {
      let base = PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/"));
      resolve_url_or_file_path_to_file_with_cache(url, &base, RemoteCacheMode::Use, &environment)
        .await
        .unwrap();

      // stale after 50 seconds because of the age, then the server says it's unchanged without an age
      environment.set_fs_time(1_050);
      environment.add_remote_file_with_headers(url, b"v1", &[("etag", "\"abc\""), ("cache-control", "max-age=300")]);
      resolve_url_or_file_path_to_file_with_cache(url, &base, RemoteCacheMode::Use, &environment)
        .await
        .unwrap();
      assert_eq!(environment.remote_file_request_count(url), 2);

      // so it's fresh for the full max age now
      environment.set_fs_time(1_349);
      resolve_url_or_file_path_to_file_with_cache(url, &base, RemoteCacheMode::Use, &environment)
        .await
        .unwrap();
      assert_eq!(environment.remote_file_request_count(url), 2);
      environment.set_fs_time(1_350);
      resolve_url_or_file_path_to_file_with_cache(url, &base, RemoteCacheMode::Use, &environment)
        .await
        .unwrap();
      assert_eq!(environment.remote_file_request_count(url), 3);
    });
  }

  #[test]
  fn should_check_server_after_default_max_age_without_caching_headers() {
    let environment = TestEnvironment::new();
    let url = "https://dprint.dev/config.json";
    environment.set_fs_time(1_000);
    environment.add_remote_file(url, b"v1");
    environment.clone().run_in_runtime(async move {
      let base = PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/"));
      resolve_url_or_file_path_to_file_with_cache(url, &base, RemoteCacheMode::Use, &environment)
        .await
        .unwrap();
      environment.add_remote_file(url, b"v2");

      environment.set_fs_time(1_000 + crate::cache::DEFAULT_MAX_AGE.as_secs() - 1);
      let result = resolve_url_or_file_path_to_file_with_cache(url, &base, RemoteCacheMode::Use, &environment)
        .await
        .unwrap();
      assert_eq!(result.content, b"v1");
      assert_eq!(environment.remote_file_request_count(url), 1);

      environment.set_fs_time(1_000 + crate::cache::DEFAULT_MAX_AGE.as_secs());
      let result = resolve_url_or_file_path_to_file_with_cache(url, &base, RemoteCacheMode::Use, &environment)
        .await
        .unwrap();
      assert_eq!(result.content, b"v2");
      assert_eq!(result.is_first_download, true);
      assert_eq!(environment.remote_file_request_count(url), 2);
    });
  }

  #[test]
  fn should_use_stale_cached_response_when_server_fails() {
    let environment = TestEnvironment::new();
    let url = "https://dprint.dev/config.json";
    environment.set_fs_time(1_000);
    environment.add_remote_file_with_headers(url, b"v1", &[("cache-control", "no-cache")]);
    environment.clone().run_in_runtime(async move {
      let base = PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/"));
      resolve_url_or_file_path_to_file_with_cache(url, &base, RemoteCacheMode::Use, &environment)
        .await
        .unwrap();

      environment.add_remote_file_error(url, "connection refused");
      let result = resolve_url_or_file_path_to_file_with_cache(url, &base, RemoteCacheMode::Use, &environment)
        .await
        .unwrap();
      assert_eq!(result.content, b"v1");
      assert_eq!(result.is_first_download, false);
      assert_eq!(
        environment.take_stderr_messages(),
        vec![format!(
          "{} Failed checking for a newer version of https://dprint.dev/config.json, so using the cached one.\n    connection refused",
          colors::yellow("Warning")
        )]
      );

      // a 404 is an answer rather than a failure though
      environment.remove_remote_file(url);
      let err = resolve_url_or_file_path_to_file_with_cache(url, &base, RemoteCacheMode::Use, &environment)
        .await
        .err()
        .unwrap();
      assert_eq!(err.to_string(), "Error downloading https://dprint.dev/config.json - 404 Not Found");
    });
  }

  #[test]
  fn should_check_server_when_reloading() {
    let environment = TestEnvironment::new();
    let url = "https://dprint.dev/config.json";
    environment.set_fs_time(1_000);
    environment.add_remote_file_with_headers(
      url,
      b"v1",
      &[("last-modified", "Sun, 06 Nov 1994 08:49:37 GMT"), ("cache-control", "max-age=300")],
    );
    environment.clone().run_in_runtime(async move {
      let base = PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/"));
      resolve_url_or_file_path_to_file_with_cache(url, &base, RemoteCacheMode::Use, &environment)
        .await
        .unwrap();

      // fresh, but reloading asks the server anyway, which says it's unchanged
      let result = resolve_url_or_file_path_to_file_with_cache(url, &base, RemoteCacheMode::Reload, &environment)
        .await
        .unwrap();
      assert_eq!(result.content, b"v1");
      assert_eq!(result.is_first_download, false);
      assert_eq!(environment.remote_file_request_count(url), 2);
      assert_eq!(
        environment.take_remote_file_cache_validators(url).unwrap(),
        "CacheValidators { etag: None, last_modified: Some(\"Sun, 06 Nov 1994 08:49:37 GMT\") }"
      );

      // changed
      environment.add_remote_file_with_headers(
        url,
        b"v2",
        &[("last-modified", "Mon, 07 Nov 1994 08:49:37 GMT"), ("cache-control", "max-age=300")],
      );
      let result = resolve_url_or_file_path_to_file_with_cache(url, &base, RemoteCacheMode::Reload, &environment)
        .await
        .unwrap();
      assert_eq!(result.content, b"v2");
      assert_eq!(result.is_first_download, true);
      assert_eq!(environment.remote_file_request_count(url), 3);

      // not reloading uses it while fresh
      let result = resolve_url_or_file_path_to_file_with_cache(url, &base, RemoteCacheMode::Use, &environment)
        .await
        .unwrap();
      assert_eq!(result.content, b"v2");
      assert_eq!(environment.remote_file_request_count(url), 3);
    });
  }

  #[test]
  fn should_check_server_via_original_url_when_redirect_target_is_stale() {
    let environment = TestEnvironment::new();
    let url = "https://example.com/config.json";
    let first_target = "https://cdn.example.com/signed-1/config.json";
    let second_target = "https://cdn.example.com/signed-2/config.json";
    environment.set_fs_time(1_000);
    environment.add_remote_file_redirect(url, first_target);
    // the redirect stays fresh for longer than its target
    environment.add_remote_file_with_headers(first_target, b"v1", &[("cache-control", "max-age=100")]);
    environment.clone().run_in_runtime(async move {
      let base = PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/"));
      let result = resolve_url_or_file_path_to_file_with_cache(url, &base, RemoteCacheMode::Use, &environment)
        .await
        .unwrap();
      assert_eq!(result.content, b"v1");
      assert_eq!(result.source.unwrap_remote().url.as_str(), first_target);

      // all fresh
      environment.set_fs_time(1_099);
      resolve_url_or_file_path_to_file_with_cache(url, &base, RemoteCacheMode::Use, &environment)
        .await
        .unwrap();
      assert_eq!(environment.remote_file_request_count(url), 1);
      assert_eq!(environment.remote_file_request_count(first_target), 1);

      // the target is stale, so the chain is requested again from the start
      // and the first target isn't requested directly
      environment.set_fs_time(1_100);
      environment.add_remote_file_redirect(url, second_target);
      environment.add_remote_file_with_headers(second_target, b"v2", &[("cache-control", "max-age=100")]);
      let result = resolve_url_or_file_path_to_file_with_cache(url, &base, RemoteCacheMode::Use, &environment)
        .await
        .unwrap();
      assert_eq!(result.content, b"v2");
      assert_eq!(result.is_first_download, true);
      assert_eq!(result.source.unwrap_remote().url.as_str(), second_target);
      assert_eq!(environment.remote_file_request_count(url), 2);
      assert_eq!(environment.remote_file_request_count(first_target), 1);
      assert_eq!(environment.remote_file_request_count(second_target), 1);

      // the new chain is fresh
      resolve_url_or_file_path_to_file_with_cache(url, &base, RemoteCacheMode::Use, &environment)
        .await
        .unwrap();
      assert_eq!(environment.remote_file_request_count(url), 2);
    });
  }

  #[test]
  fn should_not_request_fresh_redirect_target_when_only_redirect_is_stale() {
    let environment = TestEnvironment::new();
    let url = "https://example.com/config.json";
    let target = "https://cdn.example.com/config.json";
    environment.set_fs_time(1_000);
    environment.add_remote_file_redirect(url, target);
    environment.add_remote_file_with_headers(target, b"v1", &[("cache-control", "max-age=31536000")]);
    environment.clone().run_in_runtime(async move {
      let base = PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/"));
      resolve_url_or_file_path_to_file_with_cache(url, &base, RemoteCacheMode::Use, &environment)
        .await
        .unwrap();

      // the redirect has the default max age
      environment.set_fs_time(1_000 + crate::cache::DEFAULT_MAX_AGE.as_secs());
      let result = resolve_url_or_file_path_to_file_with_cache(url, &base, RemoteCacheMode::Use, &environment)
        .await
        .unwrap();
      assert_eq!(result.content, b"v1");
      assert_eq!(result.is_first_download, false);
      assert_eq!(environment.remote_file_request_count(url), 2);
      assert_eq!(environment.remote_file_request_count(target), 1);

      // reloading checks the target too
      let result = resolve_url_or_file_path_to_file_with_cache(url, &base, RemoteCacheMode::Reload, &environment)
        .await
        .unwrap();
      assert_eq!(result.content, b"v1");
      assert_eq!(environment.remote_file_request_count(url), 3);
      assert_eq!(environment.remote_file_request_count(target), 2);
    });
  }

  #[test]
  fn should_get_if_absolute_windows_file_path() {
    assert!(is_absolute_windows_file_path("C:/test"));
    assert!(is_absolute_windows_file_path("C:\\test"));
    assert!(!is_absolute_windows_file_path("C://test"));
    assert!(!is_absolute_windows_file_path("C:\\\\test"));
  }

  #[test]
  fn should_resolve_home_dir() {
    let environment = TestEnvironment::new();
    environment.clone().run_in_runtime(async move {
      let base = PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/other"));
      let cases = [
        ("~/file.json", "/home/file.json"),
        ("~/other/file.json", "/home/other/file.json"),
        ("~/a/file.json", "/home/a/file.json"),
      ];
      for (input, expected) in cases {
        environment.mk_dir_all(Path::new(expected).parent().unwrap()).unwrap();
        environment.write_file(expected, "").unwrap();
        let result = resolve_url_or_file_path_to_file_with_cache(input, &base, RemoteCacheMode::Use, &environment)
          .await
          .unwrap();
        assert_eq!(result.source.is_local(), true);
        assert_eq!(result.source.unwrap_local().path, CanonicalizedPathBuf::new_for_testing(expected));
      }
    });
  }
}
