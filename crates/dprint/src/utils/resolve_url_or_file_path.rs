use std::path::PathBuf;
use url::Url;
use crate::cache::{Cache, CreateCacheItemOptions};
use crate::environment::Environment;
use crate::types::ErrBox;

pub struct ResolvedPath {
    pub file_path: PathBuf,
    pub source: ResolvedPathSource,
    pub is_first_download: bool,
}

impl ResolvedPath {
    pub fn local(file_path: PathBuf) -> ResolvedPath {
        ResolvedPath { file_path, source: ResolvedPathSource::Local, is_first_download: false, }
    }

    pub fn remote(file_path: PathBuf, is_first_download: bool) -> ResolvedPath {
        ResolvedPath { file_path, source: ResolvedPathSource::Remote, is_first_download }
    }
}

#[derive(Debug, PartialEq)]
pub enum ResolvedPathSource {
    /// From the local file system.
    Local,
    /// From the internet.
    Remote,
}

pub async fn resolve_url_or_file_path<'a, TEnvironment : Environment>(
    url_or_file_path: &str,
    cache: &Cache<'a, TEnvironment>,
    environment: &TEnvironment,
) -> Result<ResolvedPath, ErrBox> {
    let url = Url::parse(url_or_file_path);
    if let Ok(url) = url {
        // ensure it's not a relative or data url
        if !url.cannot_be_a_base() {
            // handle file urls (ex. file:///C:/some/folder/file.json)
            if url.scheme() == "file" {
                match url.to_file_path() {
                    Ok(file_path) => return Ok(ResolvedPath::local(file_path)),
                    Err(()) => return err!("Problem converting file url `{}` to file path.", url_or_file_path),
                }
            }
            return resolve_url(&url, cache, environment).await;
        }
    }

    // assume it's a file path
    Ok(ResolvedPath::local(PathBuf::from(url_or_file_path)))
}


async fn resolve_url<'a, TEnvironment : Environment>(
    url: &Url,
    cache: &Cache<'a, TEnvironment>,
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
            bytes: &file_bytes,
            meta_data: None,
        })?
    };

    Ok(ResolvedPath::remote(cache.resolve_cache_item_file_path(&cache_item), is_first_download))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use crate::cache::Cache;
    use crate::environment::TestEnvironment;

    use super::*;

    #[tokio::test]
    async fn it_should_resolve_a_url() {
        let environment = TestEnvironment::new();
        environment.add_remote_file("https://dprint.dev/test.json", "t".as_bytes());
        let cache = Cache::new(&environment).unwrap();
        let result = resolve_url_or_file_path("https://dprint.dev/test.json", &cache, &environment).await.unwrap();
        assert_eq!(result.file_path, PathBuf::from("/cache/test.tmp"));
        assert_eq!(result.source, ResolvedPathSource::Remote);
        assert_eq!(result.is_first_download, true);
        assert_eq!(environment.read_file(&result.file_path).unwrap(), "t");

        // should get a second time from the cache
        let result = resolve_url_or_file_path("https://dprint.dev/test.json", &cache, &environment).await.unwrap();
        assert_eq!(result.file_path, PathBuf::from("/cache/test.tmp"));
        assert_eq!(result.source, ResolvedPathSource::Remote);
        assert_eq!(result.is_first_download, false);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn it_should_resolve_a_file_url_on_windows() {
        let environment = TestEnvironment::new();
        let cache = Cache::new(&environment).unwrap();
        let result = resolve_url_or_file_path("file://C:/test/test.json", &cache, &environment).await.unwrap();
        assert_eq!(result.source, ResolvedPathSource::Local);
        assert_eq!(result.file_path, PathBuf::from("C:\\test\\test.json"));
    }

    #[cfg(linux)]
    #[tokio::test]
    async fn it_should_resolve_a_file_url_on_linux() {
        let environment = TestEnvironment::new();
        let cache = Cache::new(&environment).unwrap();
        let result = resolve_url_or_file_path("file:///test/test.json", &cache, &environment).await.unwrap();
        assert_eq!(result.source, ResolvedPathSource::Local);
        assert_eq!(result.file_path, PathBuf::from("/test/test.json"));
    }


    #[tokio::test]
    async fn it_should_resolve_a_file_path() {
        let environment = TestEnvironment::new();
        let cache = Cache::new(&environment).unwrap();
        let result = resolve_url_or_file_path("test/test.json", &cache, &environment).await.unwrap();
        assert_eq!(result.source, ResolvedPathSource::Local);
        assert_eq!(result.file_path, PathBuf::from("test/test.json"));
    }

    #[tokio::test]
    async fn it_should_error_when_url_cannot_be_resolved() {
        let environment = TestEnvironment::new();
        let cache = Cache::new(&environment).unwrap();
        let err = resolve_url_or_file_path("https://dprint.dev/test.json", &cache, &environment).await.err().unwrap();
        assert_eq!(err.to_string(), "Could not find file at url https://dprint.dev/test.json");
    }
}
