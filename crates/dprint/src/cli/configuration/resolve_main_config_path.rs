use std::path::{Path, PathBuf};
use dprint_core::types::ErrBox;

use crate::cache::Cache;
use crate::cli::CliArgs;
use crate::environment::Environment;
use crate::utils::{resolve_url_or_file_path, ResolvedPath, PathSource};

const DEFAULT_CONFIG_FILE_NAME: &'static str = ".dprintrc.json";
// todo: remove this in 0.6
const ALTERNATE_CONFIG_FILE_NAME: &'static str = "dprint.config.json";

pub struct ResolvedConfigPath {
    pub resolved_path: ResolvedPath,
    pub base_path: PathBuf,
}

pub async fn resolve_main_config_path<'a, TEnvironment : Environment>(
    args: &CliArgs,
    cache: &Cache<TEnvironment>,
    environment: &TEnvironment,
) -> Result<ResolvedConfigPath, ErrBox> {
    return Ok(if let Some(config) = &args.config {
        let base_path = PathBuf::from("./"); // use cwd as base path
        let resolved_path = resolve_url_or_file_path(config, &PathSource::new_local(base_path.clone()), cache, environment).await?;
        ResolvedConfigPath {
            resolved_path,
            base_path,
        }
    } else {
        get_default_paths(environment)
    });

    fn get_default_paths(environment: &impl Environment) -> ResolvedConfigPath {
        let config_file_path = get_config_file_in_dir(&PathBuf::from("./"), environment);

        if let Some(config_file_path) = config_file_path {
            ResolvedConfigPath {
                resolved_path: ResolvedPath::local(config_file_path),
                base_path: PathBuf::from("./"),
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
        } else if let Some(path) = get_config_file_in_dir_with_name(dir, ALTERNATE_CONFIG_FILE_NAME, environment) {
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
            return Some(config_path);
        }
        None
    }
}
