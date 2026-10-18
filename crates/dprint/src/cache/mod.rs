// Lifted and adapted from code I wrote in the Deno repo.
// Copyright the Deno authors. MIT license.

use std::borrow::Cow;
use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::time::Duration;
use std::time::UNIX_EPOCH;

use serde::Deserialize;
use serde::Serialize;
use sys_traits::FsCreateDirAll;
use sys_traits::FsMetadata;
use sys_traits::FsOpen;
use sys_traits::FsRead;
use sys_traits::FsRemoveFile;
use sys_traits::FsRename;
use sys_traits::SystemRandom;
use sys_traits::SystemTimeNow;
use sys_traits::ThreadSleep;
use url::Url;

mod cache_file;

// Not exactly correct since they're not unique, but this is completely fine.
pub type HeadersMap = HashMap<String, String>;

/// How long a cached response without caching headers is used before the
/// server is checked for a newer one.
pub const DEFAULT_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// How the cached responses of remote files are used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RemoteCacheMode {
  /// Use a cached response while it's fresh, then check the server for a
  /// newer one.
  #[default]
  Use,
  /// Check the server for a newer response even when the cached one is fresh
  /// (`--reload`).
  Reload,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SerializedCachedUrlMetadata {
  pub headers: HeadersMap,
  pub url: String,
  /// Number of seconds since the UNIX epoch.
  #[serde(default)]
  pub time: Option<u64>,
}

impl SerializedCachedUrlMetadata {
  /// Whether the response can be used at the provided time (seconds since the
  /// UNIX epoch) without checking the server for a newer one, based on its
  /// caching headers and when it was cached.
  pub fn is_fresh_at(&self, now_secs: u64) -> bool {
    // responses cached before the time was stored are always checked
    let Some(cached_secs) = self.time else {
      return false;
    };
    // the response may have already spent time in a shared cache
    let age_when_cached = self.headers.get("age").and_then(|age| age.trim().parse::<u64>().ok()).unwrap_or(0);
    let current_age = now_secs.saturating_sub(cached_secs).saturating_add(age_when_cached);
    current_age < self.freshness_lifetime_secs()
  }

  /// Number of seconds the response is fresh for after being received.
  fn freshness_lifetime_secs(&self) -> u64 {
    if let Some(cache_control) = self.headers.get("cache-control") {
      let directives = cache_control.split(',').map(|directive| {
        let (name, value) = directive.split_once('=').unwrap_or((directive, ""));
        (name.trim().to_ascii_lowercase(), value.trim().trim_matches('"'))
      });
      let mut max_age = None;
      for (name, value) in directives {
        match name.as_str() {
          "no-cache" | "no-store" => return 0,
          // the first one counts and an invalid value means stale
          "max-age" if max_age.is_none() => max_age = Some(value.parse::<u64>().unwrap_or(0)),
          _ => {}
        }
      }
      if let Some(max_age) = max_age {
        return max_age;
      }
    }
    if let Some(expires) = self.headers.get("expires") {
      // an invalid date (ex. "0") means the response is already stale
      let Ok(expires) = httpdate::parse_http_date(expires) else {
        return 0;
      };
      let Some(date) = self
        .headers
        .get("date")
        .and_then(|date| httpdate::parse_http_date(date).ok())
        .or_else(|| self.time.and_then(|secs| UNIX_EPOCH.checked_add(Duration::from_secs(secs))))
      else {
        return 0;
      };
      return expires.duration_since(date).map(|duration| duration.as_secs()).unwrap_or(0);
    }
    DEFAULT_MAX_AGE.as_secs()
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheEntry {
  pub metadata: SerializedCachedUrlMetadata,
  pub content: Vec<u8>,
}

/// Computed cache key, which can help reduce the work of computing the cache key multiple times.
pub struct HttpCacheItemKey<'a> {
  pub(super) url: &'a Url,
  pub(super) file_path: PathBuf,
}

#[derive(Debug)]
struct MessagedError {
  pub message: String,
  /// The underlying I/O error.
  pub err: std::io::Error,
}

impl MessagedError {
  #[allow(clippy::new_ret_no_self)]
  pub fn new(message: String, err: std::io::Error) -> std::io::Error {
    std::io::Error::new(err.kind(), MessagedError { message, err })
  }
}

impl std::fmt::Display for MessagedError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}: {}", self.message, self.err)
  }
}

impl std::error::Error for MessagedError {}

#[sys_traits::auto_impl]
pub trait HttpCacheSys:
  FsCreateDirAll + FsMetadata + FsOpen + FsRead + FsRemoveFile + FsRename + ThreadSleep + SystemRandom + SystemTimeNow + std::fmt::Debug + Clone
{
}

#[derive(Debug)]
pub struct HttpCache<Sys: HttpCacheSys> {
  path: PathBuf,
  pub(crate) sys: Sys,
}

impl<Sys: HttpCacheSys> HttpCache<Sys> {
  pub fn new(sys: Sys, path: PathBuf) -> Self {
    Self { path, sys }
  }

  pub fn local_path_for_url(&self, url: &Url) -> std::io::Result<PathBuf> {
    Ok(self.path.join(url_to_filename(url)?))
  }

  pub fn cache_item_key<'a>(&self, url: &'a Url) -> std::io::Result<HttpCacheItemKey<'a>> {
    Ok(HttpCacheItemKey {
      url,
      file_path: self.local_path_for_url(url)?,
    })
  }

  #[cfg(test)]
  pub fn contains(&self, url: &Url) -> bool {
    let Ok(cache_filepath) = self.local_path_for_url(url) else {
      return false;
    };
    self.sys.fs_is_file(&cache_filepath).unwrap_or(false)
  }

  pub fn set(&self, url: &Url, headers: HeadersMap, content: &[u8]) -> std::io::Result<()> {
    let cache_filepath = self.local_path_for_url(url)?;
    cache_file::write(
      &self.sys,
      &cache_filepath,
      content,
      &SerializedCachedUrlMetadata {
        time: Some(self.now_secs()),
        url: url.to_string(),
        headers,
      },
    )
    .map_err(|err| MessagedError::new(format!("failed to set '{}' in the cache (maybe run `dprint clear-cache`)", url), err))?;

    Ok(())
  }

  pub fn get(&self, key: &HttpCacheItemKey) -> std::io::Result<Option<CacheEntry>> {
    cache_file::read(&self.sys, &key.file_path)
      .map_err(|err| MessagedError::new(format!("failed to get '{}' from the cache (maybe run `dprint clear-cache`)", key.url), err))
  }

  /// Writes an entry with the exact metadata, such as one an older version
  /// wrote.
  #[cfg(test)]
  pub fn set_for_testing(&self, url: &Url, content: &[u8], metadata: SerializedCachedUrlMetadata) -> std::io::Result<()> {
    cache_file::write(&self.sys, &self.local_path_for_url(url)?, content, &metadata)
  }

  /// Whether the cached response can be used now without checking the server
  /// for a newer one.
  pub fn is_fresh(&self, metadata: &SerializedCachedUrlMetadata) -> bool {
    metadata.is_fresh_at(self.now_secs())
  }

  fn now_secs(&self) -> u64 {
    self.sys.sys_time_now().duration_since(UNIX_EPOCH).unwrap().as_secs()
  }
}

/// Turn provided `url` into a hashed filename.
/// URLs can contain a lot of characters that cannot be used
/// in filenames (like "?", "#", ":"), so in order to cache
/// them properly they are deterministically hashed into ASCII
/// strings.
pub fn url_to_filename(url: &Url) -> std::io::Result<PathBuf> {
  // Replaces port part with a special string token (because
  // ":" cannot be used in filename on some platforms).
  let Some(cache_parts) = base_url_to_filename_parts(url, "_PORT") else {
    return Err(std::io::Error::new(
      ErrorKind::InvalidInput,
      format!("Can't convert url (\"{}\") to filename.", url),
    ));
  };

  let rest_str = if let Some(query) = url.query() {
    let mut rest_str = String::with_capacity(url.path().len() + 1 + query.len());
    rest_str.push_str(url.path());
    rest_str.push('?');
    rest_str.push_str(query);
    Cow::Owned(rest_str)
  } else {
    Cow::Borrowed(url.path())
  };

  // NOTE: fragment is omitted on purpose - it's not taken into
  // account when caching - it denotes parts of webpage, which
  // in case of static resources doesn't make much sense
  let hashed_filename = checksum(rest_str.as_bytes());
  let capacity = cache_parts.iter().map(|s| s.len() + 1).sum::<usize>() + 1 + hashed_filename.len();
  let mut cache_filename = PathBuf::with_capacity(capacity);
  cache_filename.extend(cache_parts.iter().map(|s| s.as_ref()));
  cache_filename.push(hashed_filename);
  debug_assert_eq!(cache_filename.capacity(), capacity);
  Ok(cache_filename)
}

pub fn base_url_to_filename_parts<'a>(url: &'a Url, port_separator: &str) -> Option<Vec<Cow<'a, str>>> {
  let mut out = Vec::with_capacity(2);

  let scheme = url.scheme();

  match scheme {
    "http" | "https" => {
      out.push(Cow::Borrowed(scheme));

      let host = url.host_str().unwrap();
      let host_port = match url.port() {
        // underscores are not allowed in domains, so adding one here is fine
        Some(port) => Cow::Owned(format!("{host}{port_separator}{port}")),
        None => Cow::Borrowed(host),
      };
      out.push(host_port);
    }
    "data" | "blob" => {
      out.push(Cow::Borrowed(scheme));
    }
    _scheme => {
      return None;
    }
  };

  Some(out)
}

pub fn checksum(v: &[u8]) -> String {
  use sha2::Digest;
  use sha2::Sha256;

  let mut hasher = Sha256::new();
  hasher.update(v);
  format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn test_url_to_filename() {
    let test_cases = [
      (
        "https://deno.land/x/foo.ts",
        "https/deno.land/2c0a064891b9e3fbe386f5d4a833bce5076543f5404613656042107213a7bbc8",
      ),
      (
        "https://deno.land:8080/x/foo.ts",
        "https/deno.land_PORT8080/2c0a064891b9e3fbe386f5d4a833bce5076543f5404613656042107213a7bbc8",
      ),
      (
        "https://deno.land/",
        "https/deno.land/8a5edab282632443219e051e4ade2d1d5bbc671c781051bf1437897cbdfea0f1",
      ),
      (
        "https://deno.land/?asdf=qwer",
        "https/deno.land/e4edd1f433165141015db6a823094e6bd8f24dd16fe33f2abd99d34a0a21a3c0",
      ),
      // should be the same as case above, fragment (#qwer) is ignored
      // when hashing
      (
        "https://deno.land/?asdf=qwer#qwer",
        "https/deno.land/e4edd1f433165141015db6a823094e6bd8f24dd16fe33f2abd99d34a0a21a3c0",
      ),
      (
        "data:application/typescript;base64,ZXhwb3J0IGNvbnN0IGEgPSAiYSI7CgpleHBvcnQgZW51bSBBIHsKICBBLAogIEIsCiAgQywKfQo=",
        "data/c21c7fc382b2b0553dc0864aa81a3acacfb7b3d1285ab5ae76da6abec213fb37",
      ),
      (
        "data:text/plain,Hello%2C%20Deno!",
        "data/967374e3561d6741234131e342bf5c6848b70b13758adfe23ee1a813a8131818",
      ),
    ];

    for (url, expected) in test_cases.iter() {
      let u = Url::parse(url).unwrap();
      let p = url_to_filename(&u).unwrap();
      assert_eq!(p, PathBuf::from(expected));
    }
  }

  #[test]
  fn test_gen() {
    let actual = checksum(b"hello world");
    assert_eq!(actual, "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9");
  }

  #[test]
  fn deserialized_no_time() {
    let json = r#"{
      "headers": {
        "content-type": "application/javascript"
      },
      "url": "https://deno.land/std/http/file_server.ts"
    }"#;
    let data: SerializedCachedUrlMetadata = serde_json::from_str(json).unwrap();
    assert_eq!(
      data,
      SerializedCachedUrlMetadata {
        headers: HeadersMap::from([("content-type".to_string(), "application/javascript".to_string())]),
        time: None,
        url: "https://deno.land/std/http/file_server.ts".to_string(),
      }
    );
  }

  #[test]
  fn serialize_deserialize_time() {
    let json = r#"{
      "headers": {
        "content-type": "application/javascript"
      },
      "url": "https://deno.land/std/http/file_server.ts",
      "time": 123456789
    }"#;
    let data: SerializedCachedUrlMetadata = serde_json::from_str(json).unwrap();
    let expected = SerializedCachedUrlMetadata {
      headers: HeadersMap::from([("content-type".to_string(), "application/javascript".to_string())]),
      time: Some(123456789),
      url: "https://deno.land/std/http/file_server.ts".to_string(),
    };
    assert_eq!(data, expected);
  }

  #[test]
  fn is_fresh_at() {
    fn metadata(time: Option<u64>, headers: &[(&str, &str)]) -> SerializedCachedUrlMetadata {
      SerializedCachedUrlMetadata {
        headers: headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        url: "https://example.com/file.json".to_string(),
        time,
      }
    }
    let day = DEFAULT_MAX_AGE.as_secs();

    // no caching headers uses the default max age
    assert!(metadata(Some(1000), &[]).is_fresh_at(1000));
    assert!(metadata(Some(1000), &[]).is_fresh_at(1000 + day - 1));
    assert!(!metadata(Some(1000), &[]).is_fresh_at(1000 + day));
    // cached before the time was stored
    assert!(!metadata(None, &[]).is_fresh_at(1000));
    // max-age
    let max_age = metadata(Some(1000), &[("cache-control", "public, max-age=300")]);
    assert!(max_age.is_fresh_at(1299));
    assert!(!max_age.is_fresh_at(1300));
    assert!(metadata(Some(1000), &[("cache-control", "Max-Age=\"300\"")]).is_fresh_at(1299));
    assert!(metadata(Some(1000), &[("cache-control", "max-age=31536000, immutable")]).is_fresh_at(1000 + day * 300));
    assert!(!metadata(Some(1000), &[("cache-control", "max-age=0, must-revalidate")]).is_fresh_at(1000));
    // an invalid max-age is stale and the first max-age counts
    assert!(!metadata(Some(1000), &[("cache-control", "max-age=-1")]).is_fresh_at(1000));
    assert!(!metadata(Some(1000), &[("cache-control", "max-age=abc")]).is_fresh_at(1000));
    assert!(metadata(Some(1000), &[("cache-control", "max-age=300, max-age=0")]).is_fresh_at(1299));
    // the age in a shared cache counts
    assert!(!metadata(Some(1000), &[("cache-control", "max-age=300"), ("age", "250")]).is_fresh_at(1050));
    assert!(metadata(Some(1000), &[("cache-control", "max-age=300"), ("age", "250")]).is_fresh_at(1049));
    // no-cache and no-store are never fresh
    assert!(!metadata(Some(1000), &[("cache-control", "no-cache")]).is_fresh_at(1000));
    assert!(!metadata(Some(1000), &[("cache-control", "max-age=300, no-store")]).is_fresh_at(1000));
    // max-age wins over expires
    assert!(metadata(Some(1000), &[("cache-control", "max-age=300"), ("expires", "0")]).is_fresh_at(1000));
    // expires relative to the date header
    let expires = metadata(
      Some(1000),
      &[("date", "Sun, 06 Nov 1994 08:49:37 GMT"), ("expires", "Sun, 06 Nov 1994 08:59:37 GMT")],
    );
    assert!(expires.is_fresh_at(1599));
    assert!(!expires.is_fresh_at(1600));
    // expires relative to the cached time without a date header
    let expires = metadata(Some(784111777), &[("expires", "Sun, 06 Nov 1994 08:59:37 GMT")]);
    assert!(expires.is_fresh_at(784111777 + 599));
    assert!(!expires.is_fresh_at(784111777 + 600));
    // expires in the past or invalid is stale
    assert!(
      !metadata(
        Some(1000),
        &[("expires", "Sun, 06 Nov 1994 08:49:37 GMT"), ("date", "Sun, 06 Nov 1994 08:59:37 GMT")]
      )
      .is_fresh_at(1000)
    );
    assert!(!metadata(Some(1000), &[("expires", "0")]).is_fresh_at(1000));
    assert!(!metadata(Some(1000), &[("expires", "-1")]).is_fresh_at(1000));
    // the clock going backwards or a corrupt time doesn't panic
    assert!(metadata(Some(1000), &[]).is_fresh_at(500));
    assert!(!metadata(Some(u64::MAX), &[("expires", "Sun, 06 Nov 1994 08:59:37 GMT")]).is_fresh_at(1000));
  }

  fn create_cache() -> HttpCache<sys_traits::impls::InMemorySys> {
    let sys = sys_traits::impls::InMemorySys::default();
    HttpCache::new(sys, PathBuf::from("/cache"))
  }

  fn test_url(path: &str) -> Url {
    Url::parse(&format!("https://example.com/{path}")).unwrap()
  }

  #[test]
  fn set_and_get() {
    let cache = create_cache();
    let url = test_url("foo.ts");
    let headers = HeadersMap::from([("content-type".to_string(), "application/typescript".to_string())]);
    cache.set(&url, headers.clone(), b"const x = 1;").unwrap();

    let key = cache.cache_item_key(&url).unwrap();
    let entry = cache.get(&key).unwrap().unwrap();
    assert_eq!(entry.content, b"const x = 1;");
    assert_eq!(entry.metadata.headers, headers);
    assert_eq!(entry.metadata.url, url.to_string());
  }

  #[test]
  fn get_missing_returns_none() {
    let cache = create_cache();
    let url = test_url("missing.ts");
    let key = cache.cache_item_key(&url).unwrap();
    let entry = cache.get(&key).unwrap();
    assert!(entry.is_none());
  }

  #[test]
  fn contains_after_set() {
    let cache = create_cache();
    let url = test_url("bar.ts");
    assert!(!cache.contains(&url));
    cache.set(&url, HeadersMap::new(), b"content").unwrap();
    assert!(cache.contains(&url));
  }

  #[test]
  fn overwrite_existing_entry() {
    let cache = create_cache();
    let url = test_url("overwrite.ts");
    cache.set(&url, HeadersMap::new(), b"old").unwrap();
    cache.set(&url, HeadersMap::new(), b"new").unwrap();

    let key = cache.cache_item_key(&url).unwrap();
    let entry = cache.get(&key).unwrap().unwrap();
    assert_eq!(entry.content, b"new");
  }

  #[test]
  fn different_urls_different_entries() {
    let cache = create_cache();
    let url_a = test_url("a.ts");
    let url_b = test_url("b.ts");
    cache.set(&url_a, HeadersMap::new(), b"aaa").unwrap();
    cache.set(&url_b, HeadersMap::new(), b"bbb").unwrap();

    let key_a = cache.cache_item_key(&url_a).unwrap();
    let key_b = cache.cache_item_key(&url_b).unwrap();
    assert_eq!(cache.get(&key_a).unwrap().unwrap().content, b"aaa");
    assert_eq!(cache.get(&key_b).unwrap().unwrap().content, b"bbb");
  }

  #[test]
  fn binary_content() {
    let cache = create_cache();
    let url = test_url("binary.wasm");
    let content: Vec<u8> = (0..=255).collect();
    cache.set(&url, HeadersMap::new(), &content).unwrap();

    let key = cache.cache_item_key(&url).unwrap();
    let entry = cache.get(&key).unwrap().unwrap();
    assert_eq!(entry.content, content);
  }
}
