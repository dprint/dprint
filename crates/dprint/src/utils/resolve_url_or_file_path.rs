use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use url::Url;

use super::PathSource;
use crate::environment::Environment;
use crate::utils::RemotePathSource;

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
  environment: &TEnvironment,
) -> Result<ResolvedFilePathWithBytes> {
  let path_source = resolve_url_or_file_path_to_path_source(url_or_file_path, base, environment)?;

  match &path_source {
    PathSource::Remote(remote_path_source) => resolve_url_to_file_with_cache(&remote_path_source.url, environment).await,
    PathSource::Local(local_path_source) => {
      let content = environment.read_file_bytes(&local_path_source.path)?;
      Ok(ResolvedFilePathWithBytes {
        source: path_source,
        is_first_download: false,
        content,
      })
    }
  }
}

async fn resolve_url_to_file_with_cache<TEnvironment: Environment>(url: &Url, environment: &TEnvironment) -> Result<ResolvedFilePathWithBytes> {
  use crate::cache::HttpCache;

  const MAX_REDIRECTS: usize = 10;

  let cache = HttpCache::new(environment.clone(), environment.get_cache_dir().join("remote"));
  let mut current_url = url.clone();

  for _ in 0..=MAX_REDIRECTS {
    let key = cache.cache_item_key(&current_url)?;

    // check cache
    if let Some(entry) = cache.get(&key)? {
      if let Some(location) = entry.metadata.headers.get("location") {
        // cached redirect — follow it
        current_url = current_url.join(location)?;
        continue;
      }
      // cached content
      let resolved_url = Url::parse(&entry.metadata.url).unwrap_or(current_url);
      return Ok(ResolvedFilePathWithBytes {
        source: PathSource::Remote(RemotePathSource { url: resolved_url }),
        is_first_download: false,
        content: entry.content,
      });
    }

    // download
    let result = environment
      .download_file_no_redirects(&current_url)
      .await?
      .ok_or_else(|| anyhow::anyhow!("Error downloading {} - 404 Not Found", url))?;

    // cache the response and ignore errors
    _ = cache.set(&current_url, result.headers.clone(), &result.content);

    // follow redirect
    if let Some(location) = result.headers.get("location") {
      current_url = current_url.join(location)?;
      continue;
    }

    return Ok(ResolvedFilePathWithBytes {
      source: PathSource::Remote(RemotePathSource { url: current_url }),
      is_first_download: true,
      content: result.content,
    });
  }

  bail!("Too many redirects for {}", url)
}

pub async fn fetch_file_or_url_bytes(url_or_file_path: &PathSource, environment: &impl Environment) -> Result<Vec<u8>> {
  match url_or_file_path {
    PathSource::Remote(path_source) => Ok(environment.download_file_err_404(&path_source.url).await?.1.content),
    PathSource::Local(path_source) => Ok(environment.read_file_bytes(&path_source.path)?),
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
      let result = resolve_url_or_file_path_to_file_with_cache(url, &base, &environment).await.unwrap();
      assert_eq!(result.source.is_remote(), true);
      assert_eq!(result.is_first_download, true);
      assert_eq!(result.content, "t".as_bytes());

      // should get a second time from the cache
      let result = resolve_url_or_file_path_to_file_with_cache(url, &base, &environment).await.unwrap();
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
      let result = resolve_url_or_file_path_to_file_with_cache("test/test.json", &base, &environment)
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
      let result = resolve_url_or_file_path_to_file_with_cache("file://C:/test/test.json", &base, &environment)
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
      let result = resolve_url_or_file_path_to_file_with_cache("file:///test/test.json", &base, &environment)
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
      let result = resolve_url_or_file_path_to_file_with_cache("C:\\test\\test.json", &base, &environment)
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
      let result = resolve_url_or_file_path_to_file_with_cache("C:/test/test.json", &base, &environment)
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
      let result = resolve_url_or_file_path_to_file_with_cache("test/test.json", &base, &environment)
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
      let result = resolve_url_or_file_path_to_file_with_cache("test/test.json", &base, &environment)
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
      let err = resolve_url_or_file_path_to_file_with_cache("https://dprint.dev/test.json", &base, &environment)
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
      let result = resolve_url_or_file_path_to_file_with_cache("https://example.com/plugin.json", &base, &environment)
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
      let result2 = resolve_url_or_file_path_to_file_with_cache("https://example.com/plugin.json", &base, &environment)
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
        let result = resolve_url_or_file_path_to_file_with_cache(input, &base, &environment).await.unwrap();
        assert_eq!(result.source.is_local(), true);
        assert_eq!(result.source.unwrap_local().path, CanonicalizedPathBuf::new_for_testing(expected));
      }
    });
  }
}
