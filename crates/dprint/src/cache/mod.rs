// Lifted and adapted from code I wrote in the Deno repo.
// Copyright the Deno authors. MIT license.

use std::borrow::Cow;
use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use serde::Deserialize;
use serde::Serialize;
use sys_traits::FsCreateDirAll;
use sys_traits::FsMetadata;
use sys_traits::FsMetadataValue;
use sys_traits::FsOpen;
use sys_traits::FsRead;
use sys_traits::FsRemoveFile;
use sys_traits::FsRename;
use sys_traits::SystemRandom;
use sys_traits::SystemTimeNow;
use sys_traits::ThreadSleep;
use thiserror::Error;
use url::Url;

mod cache_file;

// Not exactly correct since they're not unique, but this is completely fine.
pub type HeadersMap = HashMap<String, String>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SerializedCachedUrlMetadata {
  pub headers: HeadersMap,
  pub url: String,
  /// Number of seconds since the UNIX epoch.
  #[serde(default)]
  pub time: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheEntry {
  pub metadata: SerializedCachedUrlMetadata,
  pub content: Vec<u8>,
}

#[derive(Debug, Error)]
pub enum CacheReadFileError {
  #[error(transparent)]
  Io(#[from] std::io::Error),
  #[error(transparent)]
  ChecksumIntegrity(Box<ChecksumIntegrityError>),
}

/// Computed cache key, which can help reduce the work of computing the cache key multiple times.
pub struct HttpCacheItemKey<'a> {
  pub(super) url: &'a Url,
  pub(super) file_path: PathBuf,
}

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

  pub fn dir_path(&self) -> &PathBuf {
    &self.path
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

  pub fn contains(&self, url: &Url) -> bool {
    let Ok(cache_filepath) = self.local_path_for_url(url) else {
      return false;
    };
    self.sys.fs_is_file(&cache_filepath).unwrap_or(false)
  }

  pub fn read_modified_time(&self, key: &HttpCacheItemKey) -> std::io::Result<Option<SystemTime>> {
    match self.sys.fs_metadata(&key.file_path) {
      Ok(metadata) => match metadata.modified() {
        Ok(time) => Ok(Some(time)),
        Err(_) => Ok(Some(self.sys.sys_time_now())),
      },
      Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
      Err(err) => Err(err),
    }
  }

  pub fn set(&self, url: &Url, headers: HeadersMap, content: &[u8]) -> std::io::Result<()> {
    let cache_filepath = self.local_path_for_url(url)?;
    cache_file::write(
      &self.sys,
      &cache_filepath,
      content,
      &SerializedCachedUrlMetadata {
        time: Some(self.sys.sys_time_now().duration_since(UNIX_EPOCH).unwrap().as_secs()),
        url: url.to_string(),
        headers,
      },
    )?;

    Ok(())
  }

  pub fn get(&self, key: &HttpCacheItemKey, maybe_checksum: Option<Checksum>) -> Result<Option<CacheEntry>, CacheReadFileError> {
    let maybe_file = cache_file::read(&self.sys, &key.file_path)?;

    if let Some(file) = &maybe_file
      && let Some(expected_checksum) = maybe_checksum
    {
      expected_checksum.check(key.url, &file.content).map_err(CacheReadFileError::ChecksumIntegrity)?;
    }

    Ok(maybe_file)
  }

  pub fn read_headers(&self, key: &HttpCacheItemKey) -> std::io::Result<Option<HeadersMap>> {
    // targeted deserialize
    #[derive(Deserialize)]
    struct SerializedHeaders {
      pub headers: HeadersMap,
    }

    let maybe_metadata = cache_file::read_metadata::<SerializedHeaders>(&self.sys, &key.file_path)?;
    Ok(maybe_metadata.map(|m| m.headers))
  }

  pub fn read_download_time(&self, key: &HttpCacheItemKey) -> std::io::Result<Option<SystemTime>> {
    // targeted deserialize
    #[derive(Deserialize)]
    struct SerializedTime {
      pub time: Option<u64>,
    }

    let maybe_metadata = cache_file::read_metadata::<SerializedTime>(&self.sys, &key.file_path)?;
    Ok(maybe_metadata.and_then(|m| Some(SystemTime::UNIX_EPOCH + Duration::from_secs(m.time?))))
  }
}

#[derive(Debug, Error)]
#[error("Integrity check failed for {}\n\nActual: {}\nExpected: {}", .url, .actual, .expected)]
pub struct ChecksumIntegrityError {
  pub url: Url,
  pub actual: String,
  pub expected: String,
}

#[derive(Debug, Clone, Copy)]
pub struct Checksum<'a>(&'a str);

impl<'a> Checksum<'a> {
  pub fn new(checksum: &'a str) -> Self {
    Self(checksum)
  }

  pub fn as_str(&self) -> &str {
    self.0
  }

  pub fn check(&self, url: &Url, content: &[u8]) -> Result<(), Box<ChecksumIntegrityError>> {
    let actual = checksum(content);
    if self.as_str() != actual {
      Err(Box::new(ChecksumIntegrityError {
        url: url.clone(),
        expected: self.as_str().to_string(),
        actual,
      }))
    } else {
      Ok(())
    }
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
    scheme => {
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
    let entry = cache.get(&key, None).unwrap().unwrap();
    assert_eq!(entry.content, b"const x = 1;");
    assert_eq!(entry.metadata.headers, headers);
    assert_eq!(entry.metadata.url, url.to_string());
  }

  #[test]
  fn get_missing_returns_none() {
    let cache = create_cache();
    let url = test_url("missing.ts");
    let key = cache.cache_item_key(&url).unwrap();
    let entry = cache.get(&key, None).unwrap();
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
  fn read_headers() {
    let cache = create_cache();
    let url = test_url("headers.ts");
    let headers = HeadersMap::from([
      ("content-type".to_string(), "text/plain".to_string()),
      ("x-custom".to_string(), "value".to_string()),
    ]);
    cache.set(&url, headers.clone(), b"body").unwrap();

    let key = cache.cache_item_key(&url).unwrap();
    let read = cache.read_headers(&key).unwrap().unwrap();
    assert_eq!(read, headers);
  }

  #[test]
  fn read_headers_missing_returns_none() {
    let cache = create_cache();
    let url = test_url("no-headers.ts");
    let key = cache.cache_item_key(&url).unwrap();
    assert!(cache.read_headers(&key).unwrap().is_none());
  }

  #[test]
  fn read_download_time() {
    let cache = create_cache();
    let url = test_url("timed.ts");
    cache.set(&url, HeadersMap::new(), b"data").unwrap();

    let key = cache.cache_item_key(&url).unwrap();
    let time = cache.read_download_time(&key).unwrap().unwrap();
    // the time should be at or after the unix epoch
    assert!(time >= SystemTime::UNIX_EPOCH);
  }

  #[test]
  fn read_download_time_missing_returns_none() {
    let cache = create_cache();
    let url = test_url("no-time.ts");
    let key = cache.cache_item_key(&url).unwrap();
    assert!(cache.read_download_time(&key).unwrap().is_none());
  }

  #[test]
  fn overwrite_existing_entry() {
    let cache = create_cache();
    let url = test_url("overwrite.ts");
    cache.set(&url, HeadersMap::new(), b"old").unwrap();
    cache.set(&url, HeadersMap::new(), b"new").unwrap();

    let key = cache.cache_item_key(&url).unwrap();
    let entry = cache.get(&key, None).unwrap().unwrap();
    assert_eq!(entry.content, b"new");
  }

  #[test]
  fn checksum_verification_passes() {
    let cache = create_cache();
    let url = test_url("checked.ts");
    let content = b"hello world";
    cache.set(&url, HeadersMap::new(), content).unwrap();

    let expected = checksum(content);
    let key = cache.cache_item_key(&url).unwrap();
    let entry = cache.get(&key, Some(Checksum::new(&expected))).unwrap().unwrap();
    assert_eq!(entry.content, content);
  }

  #[test]
  fn checksum_verification_fails() {
    let cache = create_cache();
    let url = test_url("bad-checksum.ts");
    cache.set(&url, HeadersMap::new(), b"content").unwrap();

    let key = cache.cache_item_key(&url).unwrap();
    let result = cache.get(&key, Some(Checksum::new("bad_checksum")));
    assert!(matches!(result, Err(CacheReadFileError::ChecksumIntegrity(_))));
  }

  #[test]
  fn read_modified_time_missing() {
    let cache = create_cache();
    let url = test_url("no-mod.ts");
    let key = cache.cache_item_key(&url).unwrap();
    assert!(cache.read_modified_time(&key).unwrap().is_none());
  }

  #[test]
  fn read_modified_time_exists() {
    let cache = create_cache();
    let url = test_url("mod-time.ts");
    cache.set(&url, HeadersMap::new(), b"data").unwrap();

    let key = cache.cache_item_key(&url).unwrap();
    let time = cache.read_modified_time(&key).unwrap();
    assert!(time.is_some());
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
    assert_eq!(cache.get(&key_a, None).unwrap().unwrap().content, b"aaa");
    assert_eq!(cache.get(&key_b, None).unwrap().unwrap().content, b"bbb");
  }

  #[test]
  fn binary_content() {
    let cache = create_cache();
    let url = test_url("binary.wasm");
    let content: Vec<u8> = (0..=255).collect();
    cache.set(&url, HeadersMap::new(), &content).unwrap();

    let key = cache.cache_item_key(&url).unwrap();
    let entry = cache.get(&key, None).unwrap().unwrap();
    assert_eq!(entry.content, content);
  }
}
