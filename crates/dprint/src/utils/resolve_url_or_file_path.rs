use std::path::PathBuf;
use url::Url;
use crate::cache::{Cache, CreateCacheItemOptions};
use crate::environment::Environment;
use crate::types::ErrBox;

pub async fn resolve_url_or_file_path<'a, TEnvironment : Environment>(
    url_or_file_path: &str,
    cache: &Cache<'a, TEnvironment>,
    environment: &TEnvironment,
) -> Result<PathBuf, ErrBox> {
    let url = Url::parse(url_or_file_path);
    if let Ok(url) = url {
        // ensure it's not a relative or data url
        if !url.cannot_be_a_base() {
            // handle file urls (ex. file:///C:/some/folder/file.json)
            if url.scheme() == "file" {
                match url.to_file_path() {
                    Ok(file_path) => return Ok(file_path),
                    Err(()) => return err!("Problem converting file url `{}` to file path.", url_or_file_path),
                }
            }
            return resolve_url(&url, cache, environment).await;
        }
    }

    // assume it's a file path
    Ok(PathBuf::from(url_or_file_path))
}


async fn resolve_url<'a, TEnvironment : Environment>(
    url: &Url,
    cache: &Cache<'a, TEnvironment>,
    environment: &TEnvironment,
) -> Result<PathBuf, ErrBox> {
    let cache_key = format!("url:{}", url.as_str());

    let cache_item = if let Some(cache_item) = cache.get_cache_item(&cache_key) {
        cache_item
    } else {
        // download and save
        let file_bytes = environment.download_file(url.as_str()).await?;
        cache.create_cache_item(CreateCacheItemOptions {
            key: cache_key,
            extension: "tmp",
            bytes: &file_bytes,
            meta_data: None,
        })?
    };

    Ok(cache.resolve_cache_item_file_path(&cache_item))
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
        assert_eq!(result, PathBuf::from("/cache/test.tmp"));
        assert_eq!(environment.read_file(&result).unwrap(), "t");
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn it_should_resolve_a_file_url_on_windows() {
        let environment = TestEnvironment::new();
        let cache = Cache::new(&environment).unwrap();
        let result = resolve_url_or_file_path("file://C:/test/test.json", &cache, &environment).await.unwrap();
        assert_eq!(result, PathBuf::from("C:\\test\\test.json"));
    }

    #[cfg(linux)]
    #[tokio::test]
    async fn it_should_resolve_a_file_url_on_linux() {
        let environment = TestEnvironment::new();
        let cache = Cache::new(&environment).unwrap();
        let result = resolve_url_or_file_path("file:///test/test.json", &cache, &environment).await.unwrap();
        assert_eq!(result, PathBuf::from("/test/test.json"));
    }


    #[tokio::test]
    async fn it_should_resolve_a_file_path() {
        let environment = TestEnvironment::new();
        let cache = Cache::new(&environment).unwrap();
        let result = resolve_url_or_file_path("test/test.json", &cache, &environment).await.unwrap();
        assert_eq!(result, PathBuf::from("test/test.json"));
    }

    #[tokio::test]
    async fn it_should_error_when_url_cannot_be_resolved() {
        let environment = TestEnvironment::new();
        let cache = Cache::new(&environment).unwrap();
        let err = resolve_url_or_file_path("https://dprint.dev/test.json", &cache, &environment).await.err().unwrap();
        assert_eq!(err.to_string(), "Could not find file at url https://dprint.dev/test.json");
    }
}
