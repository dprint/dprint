use anyhow::Result;
use crossterm::style::Stylize;
use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;

use crate::arg_parser::CliArgs;
use crate::arg_parser::ConfigDiscovery;
use crate::arg_parser::SubCommand;
use crate::environment::CanonicalizedPathBuf;
use crate::environment::Environment;
use crate::utils::PathSource;
use crate::utils::ResolvedPath;
use crate::utils::resolve_url_or_file_path;

pub static POSSIBLE_CONFIG_FILE_NAMES: [&str; 4] = ["dprint.json", "dprint.jsonc", ".dprint.json", ".dprint.jsonc"];

#[derive(Debug)]
pub struct ResolvedConfigPath {
  pub resolved_path: ResolvedPath,
  pub base_path: CanonicalizedPathBuf,
  pub is_global_config: bool,
}

pub async fn resolve_main_config_path<TEnvironment: Environment>(args: &CliArgs, environment: &TEnvironment) -> Result<Option<ResolvedConfigPath>> {
  fn get_default_paths(args: &CliArgs, environment: &impl Environment) -> Result<Option<ResolvedConfigPath>> {
    let start_search_dir = get_start_search_directory(args, environment)?;
    let config_file_path = get_config_file_in_dir(&start_search_dir, environment);

    if let Some(config_file_path) = config_file_path {
      Ok(Some(ResolvedConfigPath {
        resolved_path: ResolvedPath::local(environment.canonicalize(config_file_path)?),
        base_path: start_search_dir,
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
    let base_path = environment.cwd();
    let resolved_path = resolve_url_or_file_path(config, &PathSource::new_local(base_path.clone()), environment).await?;
    Ok(Some(ResolvedConfigPath {
      resolved_path,
      base_path,
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
    && let ResolveGlobalConfigPathResult::Found(path) = resolve_global_config_path_detail(environment)
  {
    Ok(Some(path))
  } else {
    Ok(None)
  }
}

fn resolve_global_config_path_or_error(environment: &impl Environment) -> Result<ResolvedConfigPath> {
  match resolve_global_config_path_detail(environment) {
    ResolveGlobalConfigPathResult::Found(resolved_config_path) => Ok(resolved_config_path),
    ResolveGlobalConfigPathResult::NotFound => anyhow::bail!("Could not find global dprint.json file. Create one with `dprint init --global`"),
    ResolveGlobalConfigPathResult::FailedResolvingSystemDir(err) => Err(anyhow::Error::from(err).context(concat!(
      "Could not find system config directory. ",
      "Maybe specify the DPRINT_CONFIG_DIR environment ",
      "variable to say where to store the global dprint configuration file."
    ))),
  }
}

pub fn resolve_global_config_path(environment: &impl Environment) -> Option<ResolvedConfigPath> {
  match resolve_global_config_path_detail(environment) {
    ResolveGlobalConfigPathResult::Found(resolved_config_path) => Some(resolved_config_path),
    ResolveGlobalConfigPathResult::NotFound | ResolveGlobalConfigPathResult::FailedResolvingSystemDir { .. } => None,
  }
}

enum ResolveGlobalConfigPathResult {
  Found(ResolvedConfigPath),
  NotFound,
  FailedResolvingSystemDir(std::io::Error),
}

fn resolve_global_config_path_detail(environment: &impl Environment) -> ResolveGlobalConfigPathResult {
  let global_folder = match resolve_global_config_dir(environment) {
    Ok(dir) => dir,
    Err(err) => return ResolveGlobalConfigPathResult::FailedResolvingSystemDir(err),
  };
  for name in ["dprint.jsonc", "dprint.json"] {
    let file_path = global_folder.join_panic_relative(name);
    if environment.path_exists(&file_path) {
      return ResolveGlobalConfigPathResult::Found(ResolvedConfigPath {
        base_path: environment.cwd(),
        resolved_path: ResolvedPath::local(file_path),
        is_global_config: true,
      });
    }
  }
  ResolveGlobalConfigPathResult::NotFound
}

pub fn resolve_global_config_dir(environment: &impl Environment) -> std::io::Result<CanonicalizedPathBuf> {
  if let Some(folder) = resolve_env_var_folder(environment, "DPRINT_CONFIG_DIR")
    && let Ok(folder) = resolve_or_create_folder(environment, &folder).inspect_err(|err| {
      log_warn!(
        environment,
        "{} Could not resolve DPRINT_CONFIG_DIR value '{}'. Falling back to system configuration directory.",
        "Warning".yellow(),
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

pub fn get_default_config_file_in_ancestor_directories(environment: &impl Environment, start_dir: &Path) -> Result<Option<ResolvedConfigPath>> {
  for ancestor_dir in start_dir.ancestors() {
    if let Some(ancestor_config_path) = get_config_file_in_dir(ancestor_dir, environment) {
      return Ok(Some(ResolvedConfigPath {
        resolved_path: ResolvedPath::local(environment.canonicalize(ancestor_config_path)?),
        base_path: environment.canonicalize(ancestor_dir)?,
        is_global_config: false,
      }));
    }
  }

  Ok(None)
}

fn get_config_file_in_dir(dir: impl AsRef<Path>, environment: &impl Environment) -> Option<PathBuf> {
  for file_name in &POSSIBLE_CONFIG_FILE_NAMES {
    let config_path = dir.as_ref().join(file_name);
    if environment.path_exists(&config_path) {
      return Some(config_path);
    }
  }
  None
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
