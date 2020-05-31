use std::path::PathBuf;
use crate::cache::Cache;
use crate::cli::CliArgs;
use crate::environment::Environment;
use crate::types::ErrBox;
use crate::utils::{resolve_url_or_file_path, ResolvedPath};

const DEFAULT_CONFIG_FILE_NAME: &'static str = "dprint.config.json";

pub struct ResolvedConfigPath {
    pub resolved_path: ResolvedPath,
    pub base_path: PathBuf,
}

pub async fn resolve_config_path<'a, TEnvironment : Environment>(
    args: &CliArgs,
    cache: &Cache<'a, TEnvironment>,
    environment: &TEnvironment,
) -> Result<ResolvedConfigPath, ErrBox> {
    return Ok(if let Some(config) = &args.config {
        let resolved_path = resolve_url_or_file_path(config, cache, environment).await?;
        ResolvedConfigPath {
            resolved_path,
            base_path: PathBuf::from("./"), // use cwd as base path
        }
    } else {
        get_default_paths(environment)
    });

    fn get_default_paths(environment: &impl Environment) -> ResolvedConfigPath {
        let config_file_path = PathBuf::from(format!("./{}", DEFAULT_CONFIG_FILE_NAME));

        if !environment.path_exists(&config_file_path) {
            if let Some(resolved_config_path) = get_default_config_file_in_ancestor_directories(environment) {
                return resolved_config_path;
            }
        }

        // just return this if it doesn't find a config file in the ancestor paths
        ResolvedConfigPath {
            resolved_path: ResolvedPath::local(config_file_path),
            base_path: PathBuf::from("./"),
        }
    }

    fn get_default_config_file_in_ancestor_directories(environment: &impl Environment) -> Option<ResolvedConfigPath> {
        match environment.cwd() {
            Ok(cwd) => {
                for ancestor_dir in cwd.ancestors() {
                    let ancestor_config_path = ancestor_dir.join(DEFAULT_CONFIG_FILE_NAME);
                    if environment.path_exists(&ancestor_config_path) {
                        return Some(ResolvedConfigPath {
                            resolved_path: ResolvedPath::local(ancestor_config_path),
                            base_path: ancestor_dir.to_owned(),
                        });
                    }
                }
            },
            Err(err) => {
                environment.log_verbose(&format!("Error getting current working directory: {:?}", err));
            }
        }

        None
    }
}
