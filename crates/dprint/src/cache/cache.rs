use std::path::PathBuf;
use std::sync::RwLock;

use crate::environment::Environment;
use crate::types::ErrBox;
use super::manifest::*;

pub struct Cache<TEnvironment : Environment> {
    environment: TEnvironment,
    cache_manifest: RwLock<CacheManifest>,
    cache_dir_path: PathBuf,
}

pub struct CreateCacheItemOptions<'a> {
    pub key: String,
    pub extension: &'a str,
    pub bytes: &'a [u8],
    pub meta_data: Option<String>,
}

impl<TEnvironment> Cache<TEnvironment> where TEnvironment : Environment {
    pub fn new(environment: TEnvironment) -> Result<Self, ErrBox> {
        let cache_manifest = read_manifest(&environment)?;
        let cache_dir_path = environment.get_cache_dir()?;
        Ok(Cache {
            environment,
            cache_manifest: RwLock::new(cache_manifest),
            cache_dir_path,
        })
    }

    pub fn get_cache_item(&self, key: &str) -> Option<CacheItem> {
        self.cache_manifest.read().unwrap().get_item(key).map(|x| x.to_owned())
    }

    pub fn resolve_cache_item_file_path(&self, cache_item: &CacheItem) -> PathBuf {
        self.cache_dir_path.join(&cache_item.file_name)
    }

    pub fn create_cache_item<'b>(&self, options: CreateCacheItemOptions<'b>) -> Result<CacheItem, ErrBox> {
        let file_name = self.get_file_name_from_key(&options.key, &options.extension);
        let file_path = self.cache_dir_path.join(&file_name);

        let cache_item = CacheItem {
            file_name,
            created_time: self.environment.get_time_secs(),
            meta_data: options.meta_data,
        };

        self.environment.write_file_bytes(&file_path, &options.bytes)?;

        self.cache_manifest.write().unwrap().add_item(options.key, cache_item.clone());
        self.save_manifest()?;

        Ok(cache_item)
    }

    pub fn forget_item(&self, key: &str) -> Result<(), ErrBox> {
        if let Some(item) = self.cache_manifest.write().unwrap().remove_item(key) {
            let cache_file = self.cache_dir_path.join(&item.file_name);
            match self.environment.remove_file(&cache_file) {
                _ => {}, // do nothing on success or failure
            }
        } else {
            return Ok(());
        }

        // do this outside to prevent a borrow while mutably borrowing
        self.save_manifest()?;

        Ok(())
    }

    fn get_file_name_from_key(&self, key: &str, extension: &str) -> String {
        // try to get some kind of readable file name based on the key
        let mut file_name = Vec::new();
        for c in key.chars().rev() {
            if c.is_alphanumeric() || c == '-' || c == '.' {
                file_name.push(c);
            } else if !file_name.is_empty() {
                break;
            }
        }
        file_name.reverse();

        let file_name = file_name.into_iter().collect::<String>();
        let file_name = PathBuf::from(if file_name.is_empty() { String::from("temp") } else { file_name });
        let file_stem = file_name.file_stem().expect("Expected to find the file stem."); // no extension
        self.get_unique_file_name(file_stem.to_str().unwrap(), extension)
    }

    fn get_unique_file_name(&self, prefix: &str, extension: &str) -> String {
        let mut index = 1;
        loop {
            let file_name_with_ext = if index == 1 {
                get_file_name_with_ext(prefix, extension)
            } else {
                get_file_name_with_ext(&format!("{}_{}", prefix, index), extension)
            };
            if self.has_file_name_cache_item(&file_name_with_ext) {
                index += 1;
            } else {
                return file_name_with_ext;
            }
        }

        fn get_file_name_with_ext(file_name: &str, extension: &str) -> String {
            format!("{}.{}", file_name, extension)
        }
    }

    fn has_file_name_cache_item(&self, file_name: &str) -> bool {
        self.cache_manifest.read().unwrap().items().filter(|u| u.file_name == file_name).next().is_some()
    }

    fn save_manifest(&self) -> Result<(), ErrBox> {
        write_manifest(&self.cache_manifest.read().unwrap(), &self.environment)
    }
}

#[cfg(test)]
mod test {
    use crate::environment::TestEnvironment;
    use super::*;

    #[test]
    fn it_should_get_item_from_cache_manifest() {
        let environment = TestEnvironment::new();
        environment.write_file(
            &environment.get_cache_dir().unwrap().join("cache-manifest.json"),
            r#"{ "some-value": {
    "fileName": "my-file.wasm",
    "createdTime": 123456
}}"#
        ).unwrap();

        let cache = Cache::new(environment).unwrap();
        let cache_item = cache.get_cache_item("some-value").unwrap();

        assert_eq!(cache_item.file_name, "my-file.wasm");
    }

    #[test]
    fn it_should_handle_multiple_keys_with_similar_names() {
        let environment = TestEnvironment::new();

        let cache = Cache::new(environment).unwrap();
        let cache_item1 = cache.create_cache_item(CreateCacheItemOptions {
            key: String::from("prefix/test"),
            extension: "test",
            bytes: "t".as_bytes(),
            meta_data: None,
        }).unwrap();
        assert_eq!(cache_item1.file_name, "test.test");

        let cache_item2 = cache.create_cache_item(CreateCacheItemOptions {
            key: String::from("prefix2/test"),
            extension: "test",
            bytes: "t".as_bytes(),
            meta_data: None,
        }).unwrap();
        assert_eq!(cache_item2.file_name, "test_2.test");
    }

    #[test]
    fn it_should_delete_key_from_manifest_when_no_file() {
        let environment = TestEnvironment::new();
        let cache = Cache::new(environment.clone()).unwrap();
        let cache_item = cache.create_cache_item(CreateCacheItemOptions {
            key: String::from("test"),
            extension: ".test",
            bytes: "t".as_bytes(),
            meta_data: None,
        }).unwrap();

        let cache_item_file_path = cache.resolve_cache_item_file_path(&cache_item);
        environment.remove_file(&cache_item_file_path).unwrap();
        cache.forget_item("test").unwrap();

        assert_eq!(
            environment.read_file(&environment.get_cache_dir().unwrap().join("cache-manifest.json")).unwrap(),
            r#"{}"#
        );
    }

    #[test]
    fn it_should_delete_key_from_manifest_when_file_exists() {
        let environment = TestEnvironment::new();
        let cache = Cache::new(environment.clone()).unwrap();
        let cache_item = cache.create_cache_item(CreateCacheItemOptions {
            key: String::from("test"),
            extension: ".test",
            bytes: "t".as_bytes(),
            meta_data: None,
        }).unwrap();
        let cache_item_file_path = cache.resolve_cache_item_file_path(&cache_item);

        // file should exist
        assert_eq!(environment.read_file(&cache_item_file_path).is_ok(), true);

        cache.forget_item("test").unwrap();

        // should delete the file too
        assert_eq!(environment.read_file(&cache_item_file_path).is_err(), true);

        assert_eq!(
            environment.read_file(&environment.get_cache_dir().unwrap().join("cache-manifest.json")).unwrap(),
            r#"{}"#
        );
    }
}
