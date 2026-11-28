mod incremental_file;

use incremental_file::INCREMENTAL_FILE_VERSION;
pub use incremental_file::IncrementalFile;

use crate::configuration::ResolvedConfig;
use crate::environment::CanonicalizedPathBuf;
use crate::environment::Environment;
use crate::resolution::PluginsScope;
use crate::utils::get_bytes_hash;

pub struct GetIncrementalFileOptions {
  pub incremental_cli_arg: Option<bool>,
  /// Whether the cli arguments limit the run to some of the files the
  /// configuration would otherwise cover (ex. file paths were specified).
  pub is_partial_run: bool,
}

pub fn get_incremental_file<TEnvironment: Environment>(
  options: GetIncrementalFileOptions,
  config: &ResolvedConfig,
  scope: &PluginsScope<TEnvironment>,
  environment: &TEnvironment,
) -> Option<IncrementalFile<TEnvironment>> {
  let incremental_cli_arg = options.incremental_cli_arg;
  if let Some(incremental_arg) = incremental_cli_arg.or(config.incremental)
    && !incremental_arg
  {
    return None;
  }

  let base_path = config.base_path.clone();
  let file_path = get_incremental_file_path(&base_path, environment)?;
  // running from a sub directory of the config only traverses that directory
  // (see paths.rs), so that's also a partial run
  let cwd = environment.cwd();
  let is_in_sub_dir = cwd != base_path && cwd.starts_with(&base_path);
  Some(IncrementalFile::new(
    file_path,
    scope.plugins_hash(),
    options.is_partial_run || is_in_sub_dir,
    environment.clone(),
  ))
}

/// Gets the path to store the incremental file at.
///
/// This is in the `node_modules/.cache/dprint` folder of the nearest
/// `node_modules` (see `find_node_modules_parent_dir`), so the file lives with
/// the project and is cleaned up along with it. Otherwise, it's stored in the
/// cache directory with a name based on the root directory.
fn get_incremental_file_path<TEnvironment: Environment>(base_path: &CanonicalizedPathBuf, environment: &TEnvironment) -> Option<CanonicalizedPathBuf> {
  let file_path = get_node_modules_incremental_file_path(base_path, environment).or_else(|| {
    let incremental_dir = get_cache_incremental_dir(environment);
    environment.mk_dir_all(&incremental_dir).ok()?;
    Some(incremental_dir.join_panic_relative(format!("{}-v{}", get_base_path_key(base_path), INCREMENTAL_FILE_VERSION)))
  })?;
  if !environment.path_exists(&file_path) {
    remove_legacy_incremental_file(base_path, environment);
  }
  Some(file_path)
}

/// Gets the incremental file path in the nearest `node_modules` folder.
///
/// The file is named with a hash of the root directory's path relative to the
/// folder containing `node_modules` so that the config files of a workspace
/// each get their own file, and the name stays the same when the folder is
/// moved or shared between systems (ex. Windows and WSL).
fn get_node_modules_incremental_file_path<TEnvironment: Environment>(
  base_path: &CanonicalizedPathBuf,
  environment: &TEnvironment,
) -> Option<CanonicalizedPathBuf> {
  let node_modules_parent_dir = find_node_modules_parent_dir(base_path, environment)?;
  let relative_path = base_path.strip_prefix(&node_modules_parent_dir).ok()?;
  // use forward slashes so the hash is the same on every system
  let relative_path = relative_path
    .components()
    .map(|component| component.as_os_str().to_string_lossy())
    .collect::<Vec<_>>()
    .join("/");
  let incremental_dir = node_modules_parent_dir
    .join_panic_relative("node_modules")
    .join_panic_relative(".cache")
    .join_panic_relative("dprint");
  match environment.mk_dir_all(&incremental_dir) {
    Ok(()) => Some(incremental_dir.join_panic_relative(format!(
      "incremental-{}-v{}",
      get_bytes_hash(relative_path.as_bytes()),
      INCREMENTAL_FILE_VERSION
    ))),
    Err(err) => {
      log_debug!(
        environment,
        "Failed creating {}. Storing the incremental file in the cache directory instead. {:#}",
        incremental_dir.display(),
        err
      );
      None
    }
  }
}

/// Finds the nearest of the root directory and its ancestors that has a
/// `node_modules` folder (ex. the root of a workspace that hoists its
/// dependencies).
///
/// Ancestors are only searched up to the root of the git repository, so a
/// `node_modules` folder outside the project (ex. in the home directory) is
/// never used.
fn find_node_modules_parent_dir<TEnvironment: Environment>(base_path: &CanonicalizedPathBuf, environment: &TEnvironment) -> Option<CanonicalizedPathBuf> {
  let has_node_modules = |dir: &CanonicalizedPathBuf| environment.path_exists(dir.join_panic_relative("node_modules"));
  if has_node_modules(base_path) {
    return Some(base_path.clone());
  }
  let mut nearest = None;
  let mut dir = base_path.clone();
  loop {
    // a submodule or linked worktree has a .git file rather than a directory
    if environment.path_exists(dir.join_panic_relative(".git")) {
      return nearest;
    }
    dir = dir.parent()?;
    if nearest.is_none() && has_node_modules(&dir) {
      nearest = Some(dir.clone());
    }
  }
}

/// Removes the JSON incremental file that versions of dprint before the
/// binary format stored in the cache directory, which would otherwise stay
/// there forever since its name has no version.
fn remove_legacy_incremental_file<TEnvironment: Environment>(base_path: &CanonicalizedPathBuf, environment: &TEnvironment) {
  let legacy_file_path = get_cache_incremental_dir(environment).join_panic_relative(get_base_path_key(base_path));
  if environment.path_is_file(&legacy_file_path) && environment.remove_file(&legacy_file_path).is_ok() {
    log_debug!(environment, "Removed legacy incremental file {}.", legacy_file_path.display());
  }
}

fn get_cache_incremental_dir<TEnvironment: Environment>(environment: &TEnvironment) -> CanonicalizedPathBuf {
  environment.get_cache_dir().join_panic_relative("incremental")
}

fn get_base_path_key(base_path: &CanonicalizedPathBuf) -> String {
  get_bytes_hash(base_path.to_string_lossy().as_bytes()).to_string()
}
