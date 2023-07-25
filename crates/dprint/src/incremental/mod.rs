mod incremental_file;

pub use incremental_file::IncrementalFile;

use crate::configuration::ResolvedConfig;
use crate::environment::Environment;
use crate::resolution::PluginsScope;
use crate::utils::get_bytes_hash;

pub fn get_incremental_file<TEnvironment: Environment>(
  incremental_cli_arg: Option<bool>,
  config: &ResolvedConfig,
  scope: &PluginsScope<TEnvironment>,
  environment: &TEnvironment,
) -> Option<IncrementalFile<TEnvironment>> {
  if let Some(incremental_arg) = incremental_cli_arg.or(config.incremental) {
    if !incremental_arg {
      return None;
    }
  }

  // the incremental file is stored in the cache with a key based on the root directory
  let incremental_dir = environment.get_cache_dir().join_panic_relative("incremental");
  if environment.mk_dir_all(&incremental_dir).is_err() {
    return None;
  }

  let base_path = config.base_path.clone();
  let file_path = incremental_dir.join_panic_relative(get_bytes_hash(base_path.to_string_lossy().as_bytes()).to_string());
  Some(IncrementalFile::new(file_path, scope.plugins_hash(), environment.clone()))
}
