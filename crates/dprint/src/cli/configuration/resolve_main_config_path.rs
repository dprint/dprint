use std::path::{Path, PathBuf};
use dprint_core::types::ErrBox;

use crate::cache::Cache;
use crate::cli::{CliArgs, SubCommand};
use crate::environment::Environment;
use crate::utils::{resolve_url_or_file_path, ResolvedPath, PathSource};

const DEFAULT_CONFIG_FILE_NAME: &'static str = "dprint.json";
const HIDDEN_CONFIG_FILE_NAME: &'static str = ".dprint.json";
const OLD_CONFIG_FILE_NAME: &'static str = ".dprintrc.json";

pub struct ResolvedConfigPath {
    pub resolved_path: ResolvedPath,
    pub base_path: PathBuf,
}

pub fn resolve_main_config_path<'a, TEnvironment : Environment>(
    args: &CliArgs,
    cache: &Cache<TEnvironment>,
    environment: &TEnvironment,
) -> Result<ResolvedConfigPath, ErrBox> {
    return Ok(if let Some(config) = &args.config {
        let base_path = PathBuf::from("./"); // use cwd as base path
        let resolved_path = resolve_url_or_file_path(config, &PathSource::new_local(base_path.clone()), cache, environment)?;
        ResolvedConfigPath {
            resolved_path,
            base_path,
        }
    } else {
        get_default_paths(args, environment)
    });

    fn get_default_paths(args: &CliArgs, environment: &impl Environment) -> ResolvedConfigPath {
        let start_search_dir = get_start_search_directory(args);
        let config_file_path = get_config_file_in_dir(&start_search_dir, environment);

        if let Some(config_file_path) = config_file_path {
            ResolvedConfigPath {
                resolved_path: ResolvedPath::local(config_file_path),
                base_path: start_search_dir,
            }
        } else if let Some(resolved_config_path) = get_default_config_file_in_ancestor_directories(environment) {
            resolved_config_path
        } else {
            // just return this even though it doesn't exist
            ResolvedConfigPath {
                resolved_path: ResolvedPath::local(PathBuf::from(format!("./{}", DEFAULT_CONFIG_FILE_NAME))),
                base_path: PathBuf::from("./"),
            }
        }
    }

    fn get_start_search_directory(args: &CliArgs) -> PathBuf {
        if let SubCommand::StdInFmt(command) = &args.sub_command {
            // resolve the config file based on the file path provided to the command
            command.file_path.parent().map(|p| p.to_owned()).unwrap_or(PathBuf::from("./"))
        } else {
            PathBuf::from("./") // cwd
        }
    }

    fn get_default_config_file_in_ancestor_directories(environment: &impl Environment) -> Option<ResolvedConfigPath> {
        match environment.cwd() {
            Ok(cwd) => {
                for ancestor_dir in cwd.ancestors() {
                    let ancestor_dir = ancestor_dir.to_path_buf();
                    if let Some(ancestor_config_path) = get_config_file_in_dir(&ancestor_dir, environment) {
                        return Some(ResolvedConfigPath {
                            resolved_path: ResolvedPath::local(ancestor_config_path),
                            base_path: ancestor_dir,
                        });
                    }
                }
            },
            Err(err) => {
                log_verbose!(environment, "Error getting current working directory: {:?}", err);
            }
        }

        None
    }

    fn get_config_file_in_dir(dir: &Path, environment: &impl Environment) -> Option<PathBuf> {
        if let Some(path) = get_config_file_in_dir_with_name(dir, DEFAULT_CONFIG_FILE_NAME, environment) {
            Some(path)
        } else if let Some(path) = get_config_file_in_dir_with_name(dir, HIDDEN_CONFIG_FILE_NAME, environment) {
            Some(path)
        } else if let Some(path) = get_config_file_in_dir_with_name(dir, OLD_CONFIG_FILE_NAME, environment) {
            environment.log_error("WARNING: .dprintrc.json will be deprecated soon. Please rename it to dprint.json");
            Some(path)
        } else {
            None
        }
    }

    fn get_config_file_in_dir_with_name(dir: &Path, file_name: &str, environment: &impl Environment) -> Option<PathBuf> {
        let config_path = dir.join(file_name);
        if environment.path_exists(&config_path) {
            return Some(config_path);
        }
        let config_path = dir.join(format!("config/{}", file_name));
        if environment.path_exists(&config_path) {
            environment.log_error("WARNING: Automatic resolution of the configuration file in the config sub directory will be deprecated soon. Please move the configuration file to the parent directory.");
            return Some(config_path);
        }
        None
    }
}
