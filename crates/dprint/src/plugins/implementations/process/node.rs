use std::path::PathBuf;

use anyhow::Result;
use once_cell::sync::OnceCell;

use crate::environment::Environment;
use crate::utils::which_global;

const DPRINT_NODE_PATH_ENV_VAR_NAME: &str = "DPRINT_NODE_PATH";

pub fn resolve_node_executable(environment: &impl Environment) -> Result<&'static PathBuf> {
  static INSTANCE: OnceCell<Result<PathBuf>> = OnceCell::new();
  INSTANCE
    .get_or_init(|| {
      if let Some(path) = environment.var(DPRINT_NODE_PATH_ENV_VAR_NAME) {
        return Ok(PathBuf::from(path));
      }
      which_global("node", environment)
    })
    .as_ref()
    .map_err(|err| {
      anyhow::anyhow!(
        concat!(
          "The 'node' executable is required to run this plugin. Please ensure it's ",
          "installed and available on the path. Alternatively, you may supply a {} ",
          "environment variable.\n\n{:#}"
        ),
        DPRINT_NODE_PATH_ENV_VAR_NAME,
        err,
      )
    })
}

const DPRINT_NPM_PATH_ENV_VAR_NAME: &str = "DPRINT_NPM_PATH";
const DPRINT_NPM_COMMAND_ENV_VAR_NAME: &str = "DPRINT_NPM_COMMAND";

pub fn resolve_npm_executable(environment: &impl Environment) -> Result<&'static PathBuf> {
  static INSTANCE: OnceCell<Result<PathBuf>> = OnceCell::new();
  INSTANCE
    .get_or_init(|| {
      if let Some(path) = environment.var(DPRINT_NPM_PATH_ENV_VAR_NAME) {
        return Ok(PathBuf::from(path));
      }

      which_global(&npm_executable_name(environment), environment)
    })
    .as_ref()
    .map_err(|err| {
      anyhow::anyhow!(
        concat!(
          "The '{}' executable is required to run this plugin. Please ensure it's ",
          "installed and available on the path. Alternatively, you may supply a {} ",
          "or {} environment variable.\n\n{:#}"
        ),
        npm_executable_name(environment),
        DPRINT_NPM_PATH_ENV_VAR_NAME,
        DPRINT_NPM_COMMAND_ENV_VAR_NAME,
        err,
      )
    })
}

fn npm_executable_name(environment: &impl Environment) -> String {
  match environment.var(DPRINT_NPM_COMMAND_ENV_VAR_NAME) {
    Some(cmd) => cmd,
    None => "npm".to_string(),
  }
}
