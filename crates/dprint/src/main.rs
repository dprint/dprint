#[macro_use]
mod environment;

use anyhow::Result;
use dprint_core::plugins::process::setup_exit_process_panic_hook;
use environment::RealEnvironment;
use environment::RealEnvironmentOptions;
use std::sync::Arc;
use thiserror::Error;
use utils::RealStdInReader;

use crate::arg_parser::ParseArgsError;

mod arg_parser;
mod commands;
mod configuration;
mod format;
mod incremental;
mod paths;
mod patterns;
mod plugins;
mod run_cli;
mod utils;

#[cfg(test)]
mod test_helpers;

fn main() {
  setup_exit_process_panic_hook();
  let rt = tokio::runtime::Builder::new_multi_thread().enable_time().build().unwrap();
  let handle = rt.handle().clone();
  rt.block_on(async move {
    match run(handle).await {
      Ok(_) => {}
      Err(err) => {
        eprintln!("{:#}", err.inner);
        std::process::exit(err.exit_code);
      }
    }
  });
}

#[derive(Debug, Error)]
#[error("{inner:#}")]
pub struct AppError {
  pub inner: anyhow::Error,
  pub exit_code: i32,
}

impl From<anyhow::Error> for AppError {
  fn from(inner: anyhow::Error) -> Self {
    let inner = match inner.downcast::<ParseArgsError>() {
      Ok(err) => return err.into(),
      Err(err) => err,
    };
    AppError { inner, exit_code: 1 }
  }
}

impl From<ParseArgsError> for AppError {
  fn from(inner: ParseArgsError) -> Self {
    AppError {
      inner: inner.into(),
      exit_code: 2,
    }
  }
}

async fn run(runtime_handle: tokio::runtime::Handle) -> Result<(), AppError> {
  let args = arg_parser::parse_args(std::env::args().collect(), RealStdInReader)?;
  let environment = RealEnvironment::new(RealEnvironmentOptions {
    is_verbose: args.verbose,
    is_stdout_machine_readable: args.is_stdout_machine_readable(),
    runtime_handle: Arc::new(runtime_handle),
  })?;
  let plugin_cache = Arc::new(plugins::PluginCache::new(environment.clone()));
  let plugin_pools = Arc::new(plugins::PluginsCollection::new(environment.clone()));
  let plugin_resolver = plugins::PluginResolver::new(environment.clone(), plugin_cache, plugin_pools.clone());

  let result = run_cli::run_cli(&args, &environment, &plugin_resolver, plugin_pools.clone()).await;
  plugin_pools.drop_and_shutdown_initialized().await;
  Ok(result?)
}
