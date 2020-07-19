use std::path::PathBuf;
use url::Url;
use bytes::Bytes;
use crate::cache::{Cache, CreateCacheItemOptions};
use crate::environment::Environment;
use crate::types::ErrBox;

use super::PathSource;

pub struct ResolvedPath {
    pub file_path: PathBuf,
    pub source: PathSource,
    pub is_first_download: bool,
}

impl ResolvedPath {
    pub fn is_local(&self) -> bool {
        !self.is_remote()
    }

    pub fn is_remote(&self) -> bool {
        if let PathSource::Remote(_) = &self.source {
            true
        } else {
            false
        }
    }
}

impl ResolvedPath {
    pub fn local(file_path: PathBuf) -> ResolvedPath {
        ResolvedPath {
            file_path: file_path.clone(),
            source: PathSource::new_local(file_path),
            is_first_download: false,
        }
    }

    pub fn remote(file_path: PathBuf, url: Url, is_first_download: bool) -> ResolvedPath {
        ResolvedPath {
            file_path,
            source: PathSource::new_remote(url),
            is_first_download,
        }
    }
}

pub async fn resolve_url_or_file_path<TEnvironment : Environment>(
    url_or_file_path: &str,
    base: &PathSource,
    cache: &Cache<TEnvironment>,
    environment: &TEnvironment,
) -> Result<ResolvedPath, ErrBox> {
    let path_source = resolve_url_or_file_path_to_path_source(url_or_file_path, base)?;

    match path_source {
        PathSource::Remote(path_source) => {
            resolve_url(&path_source.url, cache, environment).await
        }
        PathSource::Local(path_source) => {
            Ok(ResolvedPath::local(path_source.path))
        }
    }
}

async fn resolve_url<TEnvironment : Environment>(
    url: &Url,
    cache: &Cache<TEnvironment>,
    environment: &TEnvironment,
) -> Result<ResolvedPath, ErrBox> {
    let cache_key = format!("url:{}", url.as_str());
    let mut is_first_download = false;

    let cache_item = if let Some(cache_item) = cache.get_cache_item(&cache_key) {
        cache_item
    } else {
        // download and save
        let file_bytes = environment.download_file(url.as_str()).await?;
        is_first_download = true;
        cache.create_cache_item(CreateCacheItemOptions {
            key: cache_key,
            extension: "tmp",
            bytes: Some(&file_bytes),
            meta_data: None,
        })?
    };

    Ok(ResolvedPath::remote(cache.resolve_cache_item_file_path(&cache_item), url.clone(), is_first_download))
}

pub async fn fetch_file_or_url_bytes(
    url_or_file_path: &PathSource,
    environment: &impl Environment
) -> Result<Bytes, ErrBox> {
    match url_or_file_path {
        PathSource::Remote(path_source) => {
            environment.download_file(path_source.url.as_str()).await
        }
        PathSource::Local(path_source) => {
            environment.read_file_bytes(&path_source.path)
        }
    }
}

pub fn resolve_url_or_file_path_to_path_source(
    url_or_file_path: &str,
    base: &PathSource,
) -> Result<PathSource, ErrBox> {
    let url = Url::parse(url_or_file_path);
    if let Ok(url) = url {
        if url.cannot_be_a_base() { // relative url
            if let PathSource::Remote(remote_base) = base {
                let url = remote_base.url.join(&url_or_file_path)?;
                return Ok(PathSource::new_remote(url));
            }
        } else {
            // handle file urls (ex. file:///C:/some/folder/file.json)
            if url.scheme() == "file" {
                match url.to_file_path() {
                    Ok(file_path) => return Ok(PathSource::new_local(file_path)),
                    Err(()) => return err!("Problem converting file url `{}` to file path.", url_or_file_path),
                }
            }
            return Ok(PathSource::new_remote(url));
        }
    }

    match base {
        PathSource::Remote(remote_base) => {
            let url = remote_base.url.join(&url_or_file_path)?;
            return Ok(PathSource::new_remote(url));
        }
        PathSource::Local(local_base) => {
            return Ok(PathSource::new_local(local_base.path.join(url_or_file_path)));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use crate::cache::Cache;
    use crate::environment::TestEnvironment;

    use super::super::PathSource;
    use super::*;

    #[tokio::test]
    async fn it_should_resolve_a_url() {
        let environment = TestEnvironment::new();
        environment.add_remote_file("https://dprint.dev/test.json", "t".as_bytes());
        let cache = Cache::new(environment.clone()).unwrap();
        let base = PathSource::new_local(PathBuf::from("/"));
        let result = resolve_url_or_file_path("https://dprint.dev/test.json", &base, &cache, &environment).await.unwrap();
        assert_eq!(result.file_path, PathBuf::from("/cache/test.tmp"));
        assert_eq!(result.is_remote(), true);
        assert_eq!(result.is_first_download, true);
        assert_eq!(environment.read_file(&result.file_path).unwrap(), "t");

        // should get a second time from the cache
        let result = resolve_url_or_file_path("https://dprint.dev/test.json", &base, &cache, &environment).await.unwrap();
        assert_eq!(result.file_path, PathBuf::from("/cache/test.tmp"));
        assert_eq!(result.is_remote(), true);
        assert_eq!(result.is_first_download, false);
    }

    #[tokio::test]
    async fn it_should_resolve_a_relative_path_to_base_url() {
        let environment = TestEnvironment::new();
        environment.add_remote_file("https://dprint.dev/asdf/test/test.json", "t".as_bytes());
        let cache = Cache::new(environment.clone()).unwrap();
        let base = PathSource::new_remote(Url::parse("https://dprint.dev/asdf/").unwrap());
        let result = resolve_url_or_file_path("test/test.json", &base, &cache, &environment).await.unwrap();
        assert_eq!(result.is_remote(), true);
        assert_eq!(result.file_path, PathBuf::from("/cache/test.tmp"));
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn it_should_resolve_a_file_url_on_windows() {
        let environment = TestEnvironment::new();
        let cache = Cache::new(environment.clone()).unwrap();
        let base = PathSource::new_local(PathBuf::from("C:\\"));
        let result = resolve_url_or_file_path("file://C:/test/test.json", &base, &cache, &environment).await.unwrap();
        assert_eq!(result.is_local(), true);
        assert_eq!(result.file_path, PathBuf::from("C:\\test\\test.json"));
    }

    #[cfg(linux)]
    #[tokio::test]
    async fn it_should_resolve_a_file_url_on_linux() {
        let environment = TestEnvironment::new();
        let cache = Cache::new(&environment).unwrap();
        let base = PathSource::new_local(PathBuf::from("/"));
        let result = resolve_url_or_file_path("file:///test/test.json", &base, &cache, &environment).await.unwrap();
        assert_eq!(result.is_local(), true);
        assert_eq!(result.file_path, PathBuf::from("/test/test.json"));
    }

    #[tokio::test]
    async fn it_should_resolve_a_file_path() {
        let environment = TestEnvironment::new();
        let cache = Cache::new(environment.clone()).unwrap();
        let base = PathSource::new_local(PathBuf::from("/"));
        let result = resolve_url_or_file_path("test/test.json", &base, &cache, &environment).await.unwrap();
        assert_eq!(result.is_local(), true);
        assert_eq!(result.file_path, PathBuf::from("/test/test.json"));
    }

    #[tokio::test]
    async fn it_should_resolve_a_file_path_relative_to_base_path() {
        let environment = TestEnvironment::new();
        let cache = Cache::new(environment.clone()).unwrap();
        let base = PathSource::new_local(PathBuf::from("/other"));
        let result = resolve_url_or_file_path("test/test.json", &base, &cache, &environment).await.unwrap();
        assert_eq!(result.is_local(), true);
        assert_eq!(result.file_path, PathBuf::from("/other/test/test.json"));
    }

    #[tokio::test]
    async fn it_should_error_when_url_cannot_be_resolved() {
        let environment = TestEnvironment::new();
        let cache = Cache::new(environment.clone()).unwrap();
        let base = PathSource::new_local(PathBuf::from("/other"));
        let err = resolve_url_or_file_path("https://dprint.dev/test.json", &base, &cache, &environment).await.err().unwrap();
        assert_eq!(err.to_string(), "Could not find file at url https://dprint.dev/test.json");
    }
}
