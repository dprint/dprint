use anyhow::Result;
use std::path::Path;
use std::path::PathBuf;

use crate::arg_parser::CliArgs;
use crate::arg_parser::SubCommand;
use crate::cache::Cache;
use crate::environment::CanonicalizedPathBuf;
use crate::environment::Environment;
use crate::utils::resolve_url_or_file_path;
use crate::utils::PathSource;
use crate::utils::ResolvedPath;

const DEFAULT_CONFIG_FILE_NAME: &'static str = "dprint.json";
const HIDDEN_CONFIG_FILE_NAME: &'static str = ".dprint.json";
const OLD_CONFIG_FILE_NAME: &'static str = ".dprintrc.json";

#[derive(Debug)]
pub struct ResolvedConfigPath {
  pub resolved_path: ResolvedPath,
  pub base_path: CanonicalizedPathBuf,
}

pub fn resolve_main_config_path<'a, TEnvironment: Environment>(
  args: &CliArgs,
  cache: &Cache<TEnvironment>,
  environment: &TEnvironment,
) -> Result<ResolvedConfigPath> {
  return Ok(if let Some(config) = &args.config {
    let base_path = environment.cwd();
    let resolved_path = resolve_url_or_file_path(config, &PathSource::new_local(base_path.clone()), cache, environment)?;
    ResolvedConfigPath { resolved_path, base_path }
  } else {
    get_default_paths(args, environment)?
  });

  fn get_default_paths(args: &CliArgs, environment: &impl Environment) -> Result<ResolvedConfigPath> {
    let start_search_dir = get_start_search_directory(args, environment)?;
    let config_file_path = get_config_file_in_dir(&start_search_dir, environment);

    Ok(if let Some(config_file_path) = config_file_path {
      ResolvedConfigPath {
        resolved_path: ResolvedPath::local(environment.canonicalize(config_file_path)?),
        base_path: start_search_dir,
      }
    } else if let Some(resolved_config_path) = get_default_config_file_in_ancestor_directories(environment)? {
      resolved_config_path
    } else {
      // just return this even though it doesn't exist
      ResolvedConfigPath {
        resolved_path: ResolvedPath::local(environment.cwd().join_panic_relative(DEFAULT_CONFIG_FILE_NAME)),
        base_path: environment.cwd(),
      }
    })
  }

  fn get_start_search_directory(args: &CliArgs, environment: &impl Environment) -> Result<CanonicalizedPathBuf> {
    if let SubCommand::StdInFmt(command) = &args.sub_command {
      // resolve the config file based on the file path provided to the command
      if environment.is_absolute_path(&command.file_name_or_path) {
        if let Some(parent) = PathBuf::from(&command.file_name_or_path).parent() {
          return environment.canonicalize(parent);
        }
      }
    }

    Ok(environment.cwd())
  }

  fn get_default_config_file_in_ancestor_directories(environment: &impl Environment) -> Result<Option<ResolvedConfigPath>> {
    let cwd = environment.cwd().to_path_buf();
    for ancestor_dir in cwd.ancestors() {
      if let Some(ancestor_config_path) = get_config_file_in_dir(&ancestor_dir, environment) {
        return Ok(Some(ResolvedConfigPath {
          resolved_path: ResolvedPath::local(environment.canonicalize(ancestor_config_path)?),
          base_path: environment.canonicalize(ancestor_dir)?,
        }));
      }
    }

    Ok(None)
  }

  fn get_config_file_in_dir(dir: impl AsRef<Path>, environment: &impl Environment) -> Option<PathBuf> {
    if let Some(path) = get_config_file_in_dir_with_name(&dir, DEFAULT_CONFIG_FILE_NAME, environment) {
      Some(path)
    } else if let Some(path) = get_config_file_in_dir_with_name(&dir, HIDDEN_CONFIG_FILE_NAME, environment) {
      Some(path)
    } else if let Some(path) = get_config_file_in_dir_with_name(&dir, OLD_CONFIG_FILE_NAME, environment) {
      environment.log_stderr("WARNING: .dprintrc.json will be deprecated soon. Please rename it to dprint.json");
      Some(path)
    } else {
      None
    }
  }

  fn get_config_file_in_dir_with_name(dir: impl AsRef<Path>, file_name: &str, environment: &impl Environment) -> Option<PathBuf> {
    let config_path = dir.as_ref().join(file_name);
    if environment.path_exists(&config_path) {
      return Some(config_path);
    }
    let config_path = dir.as_ref().join("config").join(file_name);
    if environment.path_exists(&config_path) {
      environment.log_stderr("WARNING: Automatic resolution of the configuration file in the config sub directory will be deprecated soon. Please move the configuration file to the parent directory.");
      return Some(config_path);
    }
    None
  }
}
