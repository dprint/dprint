use anyhow::bail;
use anyhow::Result;
use url::Url;

use super::get_bytes_hash;
use super::PathSource;
use crate::environment::CanonicalizedPathBuf;
use crate::environment::Environment;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ResolvedPath {
  pub file_path: CanonicalizedPathBuf,
  pub source: PathSource,
  pub is_first_download: bool,
}

impl ResolvedPath {
  pub fn is_local(&self) -> bool {
    !self.is_remote()
  }

  pub fn is_remote(&self) -> bool {
    matches!(&self.source, PathSource::Remote(_))
  }
}

impl ResolvedPath {
  pub fn local(file_path: CanonicalizedPathBuf) -> ResolvedPath {
    ResolvedPath {
      file_path: file_path.clone(),
      source: PathSource::new_local(file_path),
      is_first_download: false,
    }
  }

  pub fn remote(file_path: CanonicalizedPathBuf, url: Url, is_first_download: bool) -> ResolvedPath {
    ResolvedPath {
      file_path,
      source: PathSource::new_remote(url),
      is_first_download,
    }
  }
}

pub async fn resolve_url_or_file_path<TEnvironment: Environment>(
  url_or_file_path: &str,
  base: &PathSource,
  environment: &TEnvironment,
) -> Result<ResolvedPath> {
  let path_source = resolve_url_or_file_path_to_path_source(url_or_file_path, base, environment)?;

  match path_source {
    PathSource::Remote(path_source) => resolve_url(&path_source.url, environment).await,
    PathSource::Local(path_source) => Ok(ResolvedPath::local(path_source.path)),
  }
}

async fn resolve_url<TEnvironment: Environment>(url: &Url, environment: &TEnvironment) -> Result<ResolvedPath> {
  let mut is_first_download = false;

  let cache_dir = environment.get_cache_dir().join_panic_relative("remote");
  environment.mk_dir_all(&cache_dir)?;

  let url_hash = get_bytes_hash(url.as_str().as_bytes());
  let file_path = cache_dir.join_panic_relative(url_hash.to_string());

  if !environment.path_exists(&file_path) {
    is_first_download = true;
    let file_bytes = environment.download_file_err_404(url.as_str()).await?;
    let temp_path = file_path.as_ref().with_extension(".tmp");
    // atomic save
    environment.write_file_bytes(&temp_path, &file_bytes)?;
    environment.rename(&temp_path, &file_path)?;
  }

  Ok(ResolvedPath::remote(file_path, url.clone(), is_first_download))
}

pub async fn fetch_file_or_url_bytes(url_or_file_path: &PathSource, environment: &impl Environment) -> Result<Vec<u8>> {
  match url_or_file_path {
    PathSource::Remote(path_source) => environment.download_file_err_404(path_source.url.as_str()).await,
    PathSource::Local(path_source) => environment.read_file_bytes(&path_source.path),
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
  use std::path::PathBuf;

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
      let url_hash = get_bytes_hash(url.as_bytes());
      let cache_file_path = environment.get_cache_dir().join("remote").join(url_hash.to_string());
      let result = resolve_url_or_file_path(url, &base, &environment).await.unwrap();
      assert_eq!(result.file_path.as_ref(), cache_file_path);
      assert_eq!(result.is_remote(), true);
      assert_eq!(result.is_first_download, true);
      assert_eq!(environment.read_file(&result.file_path).unwrap(), "t");

      // should get a second time from the cache
      let result = resolve_url_or_file_path(url, &base, &environment).await.unwrap();
      assert_eq!(result.file_path.as_ref(), cache_file_path);
      assert_eq!(result.is_remote(), true);
      assert_eq!(result.is_first_download, false);
    });
  }

  #[test]
  fn should_resolve_a_relative_path_to_base_url() {
    let environment = TestEnvironment::new();
    environment.add_remote_file("https://dprint.dev/asdf/test/test.json", "t".as_bytes());
    environment.clone().run_in_runtime(async move {
      let base = PathSource::new_remote(Url::parse("https://dprint.dev/asdf/").unwrap());
      let result = resolve_url_or_file_path("test/test.json", &base, &environment).await.unwrap();
      assert_eq!(result.is_remote(), true);
      assert_eq!(result.file_path.as_ref(), PathBuf::from("/cache").join("remote").join("13688467613984252730"));
    });
  }

  #[cfg(windows)]
  #[test]
  fn should_resolve_a_file_url_on_windows() {
    let environment = TestEnvironment::new();
    environment.clone().run_in_runtime(async move {
      let base = PathSource::new_local(CanonicalizedPathBuf::new_for_testing("V:\\"));
      let result = resolve_url_or_file_path("file://C:/test/test.json", &base, &environment).await.unwrap();
      assert_eq!(result.is_local(), true);
      assert_eq!(result.file_path, CanonicalizedPathBuf::new_for_testing("C:\\test\\test.json"));
    });
  }

  #[cfg(unix)]
  #[test]
  fn should_resolve_a_file_url_on_unix() {
    let environment = TestEnvironment::new();
    environment.clone().run_in_runtime(async move {
      let base = PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/"));
      let result = resolve_url_or_file_path("file:///test/test.json", &base, &environment).await.unwrap();
      assert_eq!(result.is_local(), true);
      assert_eq!(result.file_path, CanonicalizedPathBuf::new_for_testing("/test/test.json"));
    });
  }

  #[cfg(windows)]
  #[test]
  fn should_resolve_an_absolute_path_on_windows() {
    let environment = TestEnvironment::new();
    environment.clone().run_in_runtime(async move {
      let base = PathSource::new_local(CanonicalizedPathBuf::new_for_testing("V:\\"));
      let result = resolve_url_or_file_path("C:\\test\\test.json", &base, &environment).await.unwrap();
      assert_eq!(result.is_local(), true);
      assert_eq!(result.file_path, CanonicalizedPathBuf::new_for_testing("C:\\test\\test.json"));
    });
  }

  #[cfg(windows)]
  #[test]
  fn should_resolve_an_absolute_path_on_windows_using_forward_slashes() {
    let environment = TestEnvironment::new();
    environment.clone().run_in_runtime(async move {
      let base = PathSource::new_local(CanonicalizedPathBuf::new_for_testing("V:\\"));
      let result = resolve_url_or_file_path("C:/test/test.json", &base, &environment).await.unwrap();
      assert_eq!(result.is_local(), true);
      assert_eq!(result.file_path, CanonicalizedPathBuf::new_for_testing("C:\\test\\test.json"));
    });
  }

  #[test]
  fn should_resolve_a_relative_file_path() {
    let environment = TestEnvironment::new();
    environment.clone().run_in_runtime(async move {
      let base = PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/"));
      let result = resolve_url_or_file_path("test/test.json", &base, &environment).await.unwrap();
      assert_eq!(result.is_local(), true);
      assert_eq!(result.file_path, CanonicalizedPathBuf::new_for_testing("/test/test.json"));
    });
  }

  #[test]
  fn should_resolve_a_file_path_relative_to_base_path() {
    let environment = TestEnvironment::new();
    environment.clone().run_in_runtime(async move {
      let base = PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/other"));
      let result = resolve_url_or_file_path("test/test.json", &base, &environment).await.unwrap();
      assert_eq!(result.is_local(), true);
      assert_eq!(result.file_path, CanonicalizedPathBuf::new_for_testing("/other/test/test.json"));
    });
  }

  #[test]
  fn should_error_when_url_cannot_be_resolved() {
    let environment = TestEnvironment::new();
    environment.clone().run_in_runtime(async move {
      let base = PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/other"));
      let err = resolve_url_or_file_path("https://dprint.dev/test.json", &base, &environment)
        .await
        .err()
        .unwrap();
      assert_eq!(err.to_string(), "Error downloading https://dprint.dev/test.json - 404 Not Found");
    });
  }

  #[test]
  fn should_get_if_absolute_windows_file_path() {
    assert!(is_absolute_windows_file_path("C:/test"));
    assert!(is_absolute_windows_file_path("C:\\test"));
    assert!(!is_absolute_windows_file_path("C://test"));
    assert!(!is_absolute_windows_file_path("C:\\\\test"));
  }
}
