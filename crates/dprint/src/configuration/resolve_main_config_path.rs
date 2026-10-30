use anyhow::Result;
use deno_terminal::colors;
use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;

use crate::arg_parser::CliArgs;
use crate::arg_parser::ConfigDiscovery;
use crate::arg_parser::SubCommand;
use crate::environment::CanonicalizedPathBuf;
use crate::environment::Environment;
use crate::utils::PathSource;
use crate::utils::ResolvedFilePathWithTextRef;
use crate::utils::resolve_url_or_file_path_to_file_with_cache;

pub static POSSIBLE_CONFIG_FILE_NAMES: [&str; 4] = ["dprint.json", "dprint.jsonc", ".dprint.json", ".dprint.jsonc"];
/// The directory a config file may be put in instead of the directory it
/// applies to in order to reduce clutter there (ex. `.config/dprint.json`).
///
/// A `.config` directory is treated as part of its parent directory: its
/// config file is found when searching the parent directory and applies to
/// the parent directory, though its anchored patterns (ex. `./src/**`) are
/// still relative to the `.config` directory. The exception is the home
/// directory's `.config`, which is where the system's configuration goes
/// (ex. `XDG_CONFIG_HOME`) and so is an ordinary directory.
pub const DOT_CONFIG_DIR_NAME: &str = ".config";
/// The config file names searched for within a `.config` directory.
pub static DOT_CONFIG_FILE_NAMES: [&str; 2] = ["dprint.json", "dprint.jsonc"];

#[derive(Debug, Clone)]
pub struct ResolvedConfigPathWithText {
  pub source: PathSource,
  pub is_first_download: bool,
  pub content: String,
  pub base_path: CanonicalizedPathBuf,
  /// The directory the config file's anchored patterns (ex. `./src/**`) are
  /// relative to when it's not the base path, which is the `.config`
  /// directory for a config file in one.
  pub anchored_patterns_base_path: Option<CanonicalizedPathBuf>,
  pub is_global_config: bool,
}

impl ResolvedConfigPathWithText {
  pub fn as_file_path_with_text_ref(&self) -> ResolvedFilePathWithTextRef<'_> {
    ResolvedFilePathWithTextRef {
      content: &self.content,
      source: &self.source,
    }
  }
}

pub async fn resolve_main_config_path_and_bytes<TEnvironment: Environment>(
  args: &CliArgs,
  environment: &TEnvironment,
) -> Result<Option<ResolvedConfigPathWithText>> {
  fn get_default_paths(args: &CliArgs, environment: &impl Environment) -> Result<Option<ResolvedConfigPathWithText>> {
    let start_search_dir = get_start_search_directory(args, environment)?;
    // a `.config` directory is searched as part of its parent directory
    let start_search_dir = match start_search_dir.parent() {
      Some(parent) if is_dot_config_dir(environment, start_search_dir.as_ref()) => parent,
      _ => start_search_dir,
    };
    let maybe_config_file = get_config_file_in_dir(&start_search_dir, environment)?;

    if let Some((path, content)) = maybe_config_file {
      let anchored_patterns_base_path = get_found_config_file_anchored_patterns_dir(&start_search_dir, start_search_dir.as_ref(), &path);
      let path = environment.canonicalize(path)?;
      Ok(Some(ResolvedConfigPathWithText {
        source: PathSource::new_local(path),
        is_first_download: false,
        content,
        base_path: start_search_dir,
        anchored_patterns_base_path,
        is_global_config: false,
      }))
    } else {
      get_default_config_file_in_ancestor_directories(environment, environment.cwd().as_ref())
    }
  }

  fn get_start_search_directory(args: &CliArgs, environment: &impl Environment) -> std::io::Result<CanonicalizedPathBuf> {
    if let SubCommand::StdInFmt(command) = &args.sub_command {
      // When formatting via stdin, resolve the config file based on the
      // file path provided to the command. This is done for people who
      // format files in their editor.
      if environment.is_absolute_path(&command.file_name_or_path)
        && let Some(parent) = PathBuf::from(&command.file_name_or_path).parent()
      {
        return environment.canonicalize(parent);
      }
    }

    Ok(environment.cwd())
  }

  let config_discovery = args.config_discovery(environment);
  if let Some(config) = &args.config {
    let cwd = environment.cwd();
    let resolved_file = resolve_url_or_file_path_to_file_with_cache(config, &PathSource::new_local(cwd.clone()), args.remote_cache_mode(), environment)
      .await?
      .into_text()?;
    // a local config file's patterns are relative to its directory, the same as
    // when it's discovered, but a remote or npm config file has no directory
    let dirs = match &resolved_file.source {
      PathSource::Local(source) => {
        // use the path as specified when it's a plain file path (see `resolve_local_config_file_dirs`)
        let is_file_path = !config.starts_with("~/") && !config.starts_with("file:");
        let specified_path = if is_file_path {
          cwd.as_ref().join(config)
        } else {
          source.path.as_ref().to_path_buf()
        };
        resolve_local_config_file_dirs(environment, &specified_path)?
      }
      PathSource::Remote(_) | PathSource::Npm(_) => LocalConfigFileDirs {
        base_path: cwd,
        anchored_patterns_base_path: None,
      },
    };
    Ok(Some(ResolvedConfigPathWithText {
      content: resolved_file.content,
      source: resolved_file.source,
      is_first_download: resolved_file.is_first_download,
      base_path: dirs.base_path,
      anchored_patterns_base_path: dirs.anchored_patterns_base_path,
      is_global_config: false,
    }))
  } else if matches!(config_discovery, ConfigDiscovery::Global) {
    resolve_global_config_path_or_error(environment).map(Some)
  } else if config_discovery.traverse_ancestors()
    && let Some(path) = get_default_paths(args, environment)?
  {
    Ok(Some(path))
  } else if matches!(config_discovery, ConfigDiscovery::Default)
    && args.plugins.is_empty()
    && let ResolveGlobalConfigPathResult::Found(path) = resolve_global_config_path_and_text_detail(environment)?
  {
    Ok(Some(path))
  } else {
    Ok(None)
  }
}

fn resolve_global_config_path_or_error(environment: &impl Environment) -> Result<ResolvedConfigPathWithText> {
  match resolve_global_config_path_and_text_detail(environment)? {
    ResolveGlobalConfigPathResult::Found(resolved_config_path) => Ok(resolved_config_path),
    ResolveGlobalConfigPathResult::NotFound => anyhow::bail!("Could not find global dprint.json file. Create one with `dprint init --global`"),
    ResolveGlobalConfigPathResult::FailedResolvingSystemDir(err) => Err(anyhow::Error::from(err).context(concat!(
      "Could not find system config directory. ",
      "Maybe specify the DPRINT_CONFIG_DIR environment ",
      "variable to say where to store the global dprint configuration file."
    ))),
  }
}

pub fn resolve_global_config_path_and_text(environment: &impl Environment) -> std::io::Result<Option<ResolvedConfigPathWithText>> {
  match resolve_global_config_path_and_text_detail(environment)? {
    ResolveGlobalConfigPathResult::Found(resolved_config_text) => Ok(Some(resolved_config_text)),
    ResolveGlobalConfigPathResult::NotFound | ResolveGlobalConfigPathResult::FailedResolvingSystemDir { .. } => Ok(None),
  }
}

/// Gets the global config file based at the root directory of the provided
/// path so the path is within the resulting config's directory. The global
/// config file is otherwise based at the cwd, and a config file only formats
/// the files within its base directory.
pub fn resolve_global_config_path_and_text_at_path_root(environment: &impl Environment, path: &Path) -> std::io::Result<Option<ResolvedConfigPathWithText>> {
  let Some(config_file) = resolve_global_config_path_and_text(environment)? else {
    return Ok(None);
  };
  // ex. the drive root on Windows
  let root_dir = path.ancestors().last().unwrap_or(path);
  Ok(Some(ResolvedConfigPathWithText {
    base_path: environment.canonicalize(root_dir)?,
    ..config_file
  }))
}

enum ResolveGlobalConfigPathResult {
  Found(ResolvedConfigPathWithText),
  NotFound,
  FailedResolvingSystemDir(std::io::Error),
}

fn resolve_global_config_path_and_text_detail(environment: &impl Environment) -> std::io::Result<ResolveGlobalConfigPathResult> {
  let global_folder = match resolve_global_config_dir(environment) {
    Ok(dir) => dir,
    Err(err) => return Ok(ResolveGlobalConfigPathResult::FailedResolvingSystemDir(err)),
  };
  for name in ["dprint.jsonc", "dprint.json"] {
    let file_path = global_folder.join_panic_relative(name);
    if let Some(content) = environment.maybe_read_file(&file_path)? {
      return Ok(ResolveGlobalConfigPathResult::Found(ResolvedConfigPathWithText {
        source: PathSource::new_local(file_path),
        is_first_download: false,
        content,
        base_path: environment.cwd(),
        anchored_patterns_base_path: None,
        is_global_config: true,
      }));
    }
  }
  Ok(ResolveGlobalConfigPathResult::NotFound)
}

pub fn resolve_global_config_dir(environment: &impl Environment) -> std::io::Result<CanonicalizedPathBuf> {
  if let Some(folder) = resolve_env_var_folder(environment, "DPRINT_CONFIG_DIR")
    && let Ok(folder) = resolve_or_create_folder(environment, &folder).inspect_err(|err| {
      log_warn!(
        environment,
        "{} Could not resolve DPRINT_CONFIG_DIR value '{}'. Falling back to system configuration directory.",
        colors::yellow("Warning"),
        folder.display(),
      );
      log_debug!(environment, "Reason: {:#}", err);
    })
  {
    Ok(folder)
  } else {
    match resolve_system_config_dir(environment) {
      Some(dir) => resolve_or_create_folder(environment, dir.join("dprint")),
      None => Err(std::io::Error::new(std::io::ErrorKind::NotFound, "Could not find system config directory.")),
    }
  }
}

/// Gets where the global config directory is without creating anything,
/// which is for reading a file that may be in it. Unlike
/// [`resolve_global_config_dir`], a `DPRINT_CONFIG_DIR` that doesn't exist is
/// still where to look, since there's nothing to fall back from until
/// something is written there.
pub fn resolve_global_config_dir_path(environment: &impl Environment) -> Option<PathBuf> {
  match resolve_env_var_folder(environment, "DPRINT_CONFIG_DIR") {
    Some(folder) => Some(PathBuf::from(folder)),
    None => resolve_system_config_dir(environment).map(|dir| dir.join("dprint")),
  }
}

fn resolve_system_config_dir(environment: &impl Environment) -> Option<PathBuf> {
  if environment.os() == "macos" {
    resolve_mac_system_config_dir(environment)
  } else {
    environment.get_config_dir()
  }
}

/// For macOS, use XDG_CONFIG_HOME if available, otherwise check if the system config directory
/// exists, but fall back to $HOME/.config if it doesn't
fn resolve_mac_system_config_dir(environment: &impl Environment) -> Option<PathBuf> {
  // first, try XDG_CONFIG_HOME
  if let Some(xdg_config_home) = resolve_env_var_folder(environment, "XDG_CONFIG_HOME") {
    return Some(PathBuf::from(xdg_config_home));
  }

  // second, check if the system config directory exists
  if let Some(config_dir) = environment.get_config_dir() {
    // check if the dprint sub dir exists
    let dprint_config = config_dir.join("dprint");
    if environment.path_exists(&dprint_config) {
      return Some(config_dir);
    }
  }

  // fall back and prefer $HOME/.config
  if let Some(home_dir) = environment.get_home_dir() {
    return Some(home_dir.join(".config"));
  }

  None
}

fn resolve_env_var_folder(environment: &impl Environment, name: &str) -> Option<OsString> {
  environment.env_var(name).filter(|f| !f.is_empty())
}

fn resolve_or_create_folder(environment: &impl Environment, path: impl AsRef<Path>) -> std::io::Result<CanonicalizedPathBuf> {
  match environment.canonicalize(path.as_ref()) {
    Ok(path) => Ok(path),
    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
      if let (Some(parent), Some(filename)) = (path.as_ref().parent(), path.as_ref().file_name()) {
        _ = environment.mk_dir_all(parent);
        Ok(environment.canonicalize(parent)?.join_panic_relative(filename.to_string_lossy()))
      } else {
        Err(err)
      }
    }
    Err(err) => Err(err),
  }
}

pub fn get_default_config_file_in_ancestor_directories(environment: &impl Environment, start_dir: &Path) -> Result<Option<ResolvedConfigPathWithText>> {
  // a `.config` directory is searched as part of its parent directory, which is next
  for ancestor_dir in start_dir.ancestors().filter(|dir| !is_dot_config_dir(environment, dir)) {
    if let Some((ancestor_config_path, content)) = get_config_file_in_dir(ancestor_dir, environment)? {
      let base_path = environment.canonicalize(ancestor_dir)?;
      return Ok(Some(ResolvedConfigPathWithText {
        source: PathSource::new_local(environment.canonicalize(&ancestor_config_path)?),
        is_first_download: false,
        content,
        anchored_patterns_base_path: get_found_config_file_anchored_patterns_dir(&base_path, ancestor_dir, &ancestor_config_path),
        base_path,
        is_global_config: false,
      }));
    }
  }

  Ok(None)
}

/// Gets if the directory is a `.config` directory, which is treated as part
/// of its parent directory (see [`DOT_CONFIG_DIR_NAME`]).
pub fn is_dot_config_dir(environment: &impl Environment, dir: &Path) -> bool {
  dir.file_name().is_some_and(|name| name == DOT_CONFIG_DIR_NAME) && dir.parent().is_some_and(|parent| !is_home_dir(environment, parent))
}

/// The directories of a local config file.
pub struct LocalConfigFileDirs {
  /// The directory the config file applies to.
  pub base_path: CanonicalizedPathBuf,
  /// The directory the config file's anchored patterns are relative to when
  /// it's not the base path (see [`ResolvedConfigPathWithText`]).
  pub anchored_patterns_base_path: Option<CanonicalizedPathBuf>,
}

/// Resolves the directories of a local config file from the path it was
/// found at or specified with.
///
/// This uses the path before it's canonicalized because whether the config
/// file is in a `.config` directory depends on where it was found, which the
/// canonicalized path doesn't say when the config file or `.config` directory
/// is a symlink (or on a case insensitive file system).
pub fn resolve_local_config_file_dirs(environment: &impl Environment, config_path: &Path) -> std::io::Result<LocalConfigFileDirs> {
  match get_dot_config_file_base_dir(environment, config_path) {
    Some(base_dir) => {
      let base_path = environment.canonicalize(base_dir)?;
      Ok(LocalConfigFileDirs {
        anchored_patterns_base_path: Some(base_path.join_panic_relative(DOT_CONFIG_DIR_NAME)),
        base_path,
      })
    }
    None => {
      let config_path = environment.canonicalize(config_path)?;
      Ok(LocalConfigFileDirs {
        base_path: config_path.parent().unwrap_or_else(|| config_path.clone()),
        anchored_patterns_base_path: None,
      })
    }
  }
}

/// Gets the directory a config file in a `.config` directory applies to,
/// which is the parent of the `.config` directory.
pub fn get_dot_config_file_base_dir<'a>(environment: &impl Environment, config_path: &'a Path) -> Option<&'a Path> {
  let file_name = config_path.file_name()?.to_str()?;
  let dir = config_path.parent()?;
  if DOT_CONFIG_FILE_NAMES.contains(&file_name) && is_dot_config_dir(environment, dir) {
    dir.parent()
  } else {
    None
  }
}

/// Gets the paths a config file may be at within the `.config` directory of
/// the provided directory, which is empty when there's no `.config` directory.
///
/// Checking for the `.config` directory first means a directory without one,
/// which is most of them, only costs a single stat.
pub fn get_dot_config_file_paths(environment: &impl Environment, dir: &Path) -> Vec<PathBuf> {
  let dot_config_dir = dir.join(DOT_CONFIG_DIR_NAME);
  if environment.path_exists(&dot_config_dir) && is_dot_config_dir(environment, &dot_config_dir) {
    DOT_CONFIG_FILE_NAMES.iter().map(|name| dot_config_dir.join(name)).collect()
  } else {
    Vec::new()
  }
}

fn get_config_file_in_dir(dir: impl AsRef<Path>, environment: &impl Environment) -> std::io::Result<Option<(PathBuf, String)>> {
  for file_name in &POSSIBLE_CONFIG_FILE_NAMES {
    let config_path = dir.as_ref().join(file_name);
    if let Some(text) = environment.maybe_read_file(&config_path)? {
      return Ok(Some((config_path, text)));
    }
  }
  for config_path in get_dot_config_file_paths(environment, dir.as_ref()) {
    // check it's a file first because reading within a `.config` that's
    // not a directory errors on some platforms rather than not finding it
    if environment.path_is_file(&config_path)
      && let Some(text) = environment.maybe_read_file(&config_path)?
    {
      return Ok(Some((config_path, text)));
    }
  }
  Ok(None)
}

/// Gets the directory the anchored patterns of a config file found in the
/// provided directory by [`get_config_file_in_dir`] are relative to, which is
/// its `.config` directory when that's where the config file was found.
fn get_found_config_file_anchored_patterns_dir(base_path: &CanonicalizedPathBuf, dir: &Path, config_path: &Path) -> Option<CanonicalizedPathBuf> {
  (config_path.parent() != Some(dir)).then(|| base_path.join_panic_relative(DOT_CONFIG_DIR_NAME))
}

fn is_home_dir(environment: &impl Environment, dir: &Path) -> bool {
  let Some(home_dir) = environment.get_home_dir() else {
    return false;
  };
  // the directory might not be canonicalized (ex. from a uri in the lsp), but
  // only canonicalize when necessary since this is called for every `.config`
  home_dir.as_ref() == dir || environment.canonicalize(dir).is_ok_and(|dir| dir == home_dir)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::environment::TestEnvironment;

  #[test]
  fn test_resolve_system_config_dir_macos_with_xdg_config_home() {
    let env = TestEnvironment::new();
    env.set_os("macos");
    env.set_env_var("XDG_CONFIG_HOME", Some("/custom/xdg/config"));

    let result = resolve_system_config_dir(&env);
    assert_eq!(result, Some(PathBuf::from("/custom/xdg/config")));
  }

  #[test]
  fn test_resolve_system_config_dir_macos_with_empty_xdg_config_home() {
    let env = TestEnvironment::new();
    env.set_os("macos");
    env.set_env_var("XDG_CONFIG_HOME", Some(""));

    // Empty XDG_CONFIG_HOME should be ignored, fall back to $HOME/.config
    let result = resolve_system_config_dir(&env);
    assert_eq!(result, Some(PathBuf::from("/home/.config")));
  }

  #[test]
  fn test_resolve_system_config_dir_macos_with_existing_dprint_dir() {
    let env = TestEnvironment::new();
    env.set_os("macos");
    env.mk_dir_all("/config/dprint").unwrap();

    let result = resolve_system_config_dir(&env);
    assert_eq!(result, Some(PathBuf::from("/config")));
  }

  #[test]
  fn test_resolve_system_config_dir_macos_without_existing_dprint_dir() {
    let env = TestEnvironment::new();
    env.set_os("macos");
    // Don't create /config/dprint

    // Should fall back to $HOME/.config
    let result = resolve_system_config_dir(&env);
    assert_eq!(result, Some(PathBuf::from("/home/.config")));
  }

  #[test]
  fn test_resolve_system_config_dir_macos_priority_xdg_over_existing() {
    let env = TestEnvironment::new();
    env.set_os("macos");
    env.set_env_var("XDG_CONFIG_HOME", Some("/custom/xdg"));
    env.mk_dir_all("/config/dprint").unwrap();

    // XDG_CONFIG_HOME should take priority even if dprint dir exists
    let result = resolve_system_config_dir(&env);
    assert_eq!(result, Some(PathBuf::from("/custom/xdg")));
  }

  #[test]
  fn test_resolve_system_config_dir_linux() {
    let env = TestEnvironment::new();
    env.set_os("linux");

    // Non-macOS should use config_dir
    let result = resolve_system_config_dir(&env);
    assert_eq!(result, Some(PathBuf::from("/config")));
  }

  #[test]
  fn test_resolve_system_config_dir_windows() {
    let env = TestEnvironment::new();
    env.set_os("windows");

    // Non-macOS should use config_dir
    let result = resolve_system_config_dir(&env);
    assert_eq!(result, Some(PathBuf::from("/config")));
  }

  #[test]
  fn test_resolve_mac_system_config_dir_xdg_priority() {
    let env = TestEnvironment::new();
    env.set_env_var("XDG_CONFIG_HOME", Some("/xdg"));
    env.mk_dir_all("/config/dprint").unwrap();

    let result = resolve_mac_system_config_dir(&env);
    assert_eq!(result, Some(PathBuf::from("/xdg")));
  }

  #[test]
  fn test_resolve_mac_system_config_dir_existing_dprint_priority() {
    let env = TestEnvironment::new();
    env.mk_dir_all("/config/dprint").unwrap();

    let result = resolve_mac_system_config_dir(&env);
    assert_eq!(result, Some(PathBuf::from("/config")));
  }

  #[test]
  fn test_resolve_mac_system_config_dir_home_fallback() {
    let env = TestEnvironment::new();
    // No XDG_CONFIG_HOME, no existing dprint dir

    let result = resolve_mac_system_config_dir(&env);
    assert_eq!(result, Some(PathBuf::from("/home/.config")));
  }

  #[test]
  fn test_resolve_env_var_folder_with_value() {
    let env = TestEnvironment::new();
    env.set_env_var("TEST_VAR", Some("/some/path"));

    let result = resolve_env_var_folder(&env, "TEST_VAR");
    assert_eq!(result, Some(OsString::from("/some/path")));
  }

  #[test]
  fn test_resolve_env_var_folder_with_empty_value() {
    let env = TestEnvironment::new();
    env.set_env_var("TEST_VAR", Some(""));

    let result = resolve_env_var_folder(&env, "TEST_VAR");
    assert_eq!(result, None);
  }

  #[test]
  fn test_resolve_env_var_folder_not_set() {
    let env = TestEnvironment::new();

    let result = resolve_env_var_folder(&env, "NONEXISTENT_VAR");
    assert_eq!(result, None);
  }
}
