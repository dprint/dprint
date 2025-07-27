use anyhow::Result;
use std::path::Path;
use std::path::PathBuf;

use crate::arg_parser::CliArgs;
use crate::arg_parser::SubCommand;
use crate::environment::CanonicalizedPathBuf;
use crate::environment::Environment;
use crate::utils::resolve_url_or_file_path;
use crate::utils::PathSource;
use crate::utils::ResolvedPath;

const DEFAULT_CONFIG_FILE_NAME: &str = "dprint.json";
const POSSIBLE_CONFIG_FILE_NAMES: [&str; 4] = [DEFAULT_CONFIG_FILE_NAME, "dprint.jsonc", ".dprint.json", ".dprint.jsonc"];

#[derive(Debug)]
pub struct ResolvedConfigPath {
  pub resolved_path: ResolvedPath,
  pub base_path: CanonicalizedPathBuf,
}

pub async fn resolve_main_config_path(args: &CliArgs, environment: &impl Environment) -> Result<Option<ResolvedConfigPath>> {
  let is_fallback = args.config_precedence();
  if is_fallback.is_prefer_file() {
    // If passed the `--config-precedence=prefer-file` flag, we will first
    // try to find local config, then maybe using the `--config` flag
    if args.config_discovery(environment).traverse_ancestors() {
      if let Some(local_path) = get_default_paths(args, environment)? {
        return Ok(Some(local_path));
      } else if let Some(config) = &args.config {
        return resolve_config_arg(config, environment).await.map(Some);
      } else {
        return Ok(None);
      }
    }
  }
  return Ok(if let Some(config) = &args.config {
    return resolve_config_arg(config, environment).await.map(Some);
  } else if args.config_discovery(environment).traverse_ancestors() {
    get_default_paths(args, environment)?
  } else {
    None
  });

  async fn resolve_config_arg(specified_config: &str, environment: &impl Environment) -> Result<ResolvedConfigPath> {
    let base_path = environment.cwd();
    let resolved_path = resolve_url_or_file_path(specified_config, &PathSource::new_local(base_path.clone()), environment).await?;
    Ok(ResolvedConfigPath { resolved_path, base_path })
  }

  fn get_default_paths(args: &CliArgs, environment: &impl Environment) -> Result<Option<ResolvedConfigPath>> {
    let start_search_dir = get_start_search_directory(args, environment)?;
    let config_file_path = get_config_file_in_dir(&start_search_dir, environment);

    if let Some(config_file_path) = config_file_path {
      Ok(Some(ResolvedConfigPath {
        resolved_path: ResolvedPath::local(environment.canonicalize(config_file_path)?),
        base_path: start_search_dir,
      }))
    } else {
      get_default_config_file_in_ancestor_directories(environment, environment.cwd().as_ref())
    }
  }

  fn get_start_search_directory(args: &CliArgs, environment: &impl Environment) -> Result<CanonicalizedPathBuf> {
    if let SubCommand::StdInFmt(command) = &args.sub_command {
      // When formatting via stdin, resolve the config file based on the
      // file path provided to the command. This is done for people who
      // format files in their editor.
      if environment.is_absolute_path(&command.file_name_or_path) {
        if let Some(parent) = PathBuf::from(&command.file_name_or_path).parent() {
          return environment.canonicalize(parent);
        }
      }
    }

    Ok(environment.cwd())
  }
}

pub fn get_default_config_file_in_ancestor_directories(environment: &impl Environment, start_dir: &Path) -> Result<Option<ResolvedConfigPath>> {
  for ancestor_dir in start_dir.ancestors() {
    if let Some(ancestor_config_path) = get_config_file_in_dir(ancestor_dir, environment) {
      return Ok(Some(ResolvedConfigPath {
        resolved_path: ResolvedPath::local(environment.canonicalize(ancestor_config_path)?),
        base_path: environment.canonicalize(ancestor_dir)?,
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
