mod incremental_file;

pub use incremental_file::IncrementalFile;

use std::sync::Arc;

use crate::cache::Cache;
use crate::cache::CreateCacheItemOptions;
use crate::configuration::ResolvedConfig;
use crate::environment::Environment;
use crate::plugins::PluginsCollection;

pub fn get_incremental_file<TEnvironment: Environment>(
  incremental_cli_arg: Option<bool>,
  config: &ResolvedConfig,
  cache: &Cache<TEnvironment>,
  plugin_pools: &PluginsCollection<TEnvironment>,
  environment: &TEnvironment,
) -> Option<Arc<IncrementalFile<TEnvironment>>> {
  if let Some(incremental_arg) = incremental_cli_arg.or(config.incremental) {
    if !incremental_arg {
      return None;
    }
  }

  // the incremental file is stored in the cache with a key based on the root directory
  let base_path = config.base_path.clone();
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
        environment.log_stderr(&format!("Could not create cache item for incremental feature. {:#}", err));
        return None;
      }
    }
  };
  let file_path = cache.resolve_cache_item_file_path(&cache_item);
  Some(Arc::new(IncrementalFile::new(
    file_path,
    plugin_pools.get_plugins_hash(),
    environment.clone(),
    base_path,
  )))
}
