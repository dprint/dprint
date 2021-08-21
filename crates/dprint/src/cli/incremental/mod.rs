mod incremental_file;

pub use incremental_file::IncrementalFile;

use std::sync::Arc;

use crate::cache::{Cache, CreateCacheItemOptions};
use crate::environment::Environment;
use crate::plugins::PluginPools;

use super::CliArgs;
use super::configuration::ResolvedConfig;

pub fn get_incremental_file<TEnvironment: Environment>(
  args: &CliArgs,
  config: &ResolvedConfig,
  cache: &Cache<TEnvironment>,
  plugin_pools: &PluginPools<TEnvironment>,
  environment: &TEnvironment,
) -> Option<Arc<IncrementalFile<TEnvironment>>> {
  if args.incremental || config.incremental {
      // the incremental file is stored in the cache with a key based on the root directory
      let base_path = match environment.canonicalize(&config.base_path) {
          Ok(base_path) => base_path,
          Err(err) => {
              environment.log_error(&format!("Could not canonicalize base path for incremental feature. {}", err));
              return None;
          }
      };
      let key = format!("incremental_cache:{}", base_path.to_string_lossy());
      let cache_item = if let Some(cache_item) = cache.get_cache_item(&key) {
          cache_item
      } else {
          let cache_item = cache.create_cache_item(CreateCacheItemOptions {
              key,
              extension: "incremental",
              bytes: None,
              meta_data: None,
          });
          match cache_item {
              Ok(cache_item) => cache_item,
              Err(err) => {
                  environment.log_error(&format!("Could not create cache item for incremental feature. {}", err));
                  return None;
              }
          }
      };
      let file_path = cache.resolve_cache_item_file_path(&cache_item);
      Some(Arc::new(IncrementalFile::new(file_path, plugin_pools.get_plugins_hash(), environment.clone(), base_path)))
  } else {
      None
  }
}
