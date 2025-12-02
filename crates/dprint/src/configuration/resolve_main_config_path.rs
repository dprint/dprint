use anyhow::Result;
use crossterm::style::Stylize;
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

const DEFAULT_CONFIG_FILE_NAME: &str = "dprint.json";
const POSSIBLE_CONFIG_FILE_NAMES: [&str; 4] = [DEFAULT_CONFIG_FILE_NAME, "dprint.jsonc", ".dprint.json", ".dprint.jsonc"];

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
    && let ResolveGlobalConfigPathResult::Found(path) = resolve_global_config_path(environment)
  {
    Ok(Some(path))
  } else {
    Ok(None)
  }
}

fn resolve_global_config_path_or_error(environment: &impl Environment) -> Result<ResolvedConfigPath> {
  match resolve_global_config_path(environment) {
    ResolveGlobalConfigPathResult::Found(resolved_config_path) => Ok(resolved_config_path),
    ResolveGlobalConfigPathResult::NotFound => anyhow::bail!("Could not find global dprint.json file. Create one with `dprint init --global`"),
    ResolveGlobalConfigPathResult::FailedResolvingSystemDir => anyhow::bail!(concat!(
      "Could not find system config directory. ",
      "Maybe specify the DPRINT_CONFIG_DIR environment ",
      "variable to say where to store the global dprint configuration file."
    )),
  }
}

enum ResolveGlobalConfigPathResult {
  Found(ResolvedConfigPath),
  NotFound,
  FailedResolvingSystemDir,
}

fn resolve_global_config_path(environment: &impl Environment) -> ResolveGlobalConfigPathResult {
  let Some(global_folder) = resolve_global_config_dir(environment) else {
    return ResolveGlobalConfigPathResult::FailedResolvingSystemDir;
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

pub fn resolve_global_config_dir(environment: &impl Environment) -> Option<CanonicalizedPathBuf> {
  fn resolve_or_create_folder(environment: &impl Environment, path: impl AsRef<Path>) -> Result<CanonicalizedPathBuf> {
    match environment.canonicalize(path.as_ref()) {
      Ok(path) => Ok(path),
      Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
        if let Some(parent) = path.as_ref().parent() {
          _ = environment.mk_dir_all(parent);
          Ok(environment.canonicalize(path)?)
        } else {
          Err(err.into())
        }
      }
      Err(err) => Err(err.into()),
    }
  }

  if let Some(folder) = environment.env_var("DPRINT_CONFIG_DIR")
    && !folder.is_empty()
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
    Some(folder)
  } else {
    environment.get_config_dir().map(|dir| dir.join_panic_relative("dprint"))
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
