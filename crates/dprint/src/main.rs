#[macro_use]
mod environment;

use anyhow::Result;
use dprint_core::plugins::process::setup_exit_process_panic_hook;
use environment::RealEnvironment;
use environment::RealEnvironmentOptions;
use run_cli::AppError;
use std::sync::Arc;
use utils::RealStdInReader;

mod arg_parser;
mod commands;
mod configuration;
mod factory;
mod format;
mod incremental;
mod paths;
mod patterns;
mod plugins;
mod resolution;
mod run_cli;
mod scope;
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

async fn run(runtime_handle: tokio::runtime::Handle) -> Result<(), AppError> {
  let args = arg_parser::parse_args(std::env::args().collect(), RealStdInReader)?;

  let environment = RealEnvironment::new(RealEnvironmentOptions {
    is_verbose: args.verbose,
    is_stdout_machine_readable: args.is_stdout_machine_readable(),
    runtime_handle: Arc::new(runtime_handle),
  })?;
  let plugin_cache = Arc::new(plugins::PluginCache::new(environment.clone()));
  let plugin_resolver = Arc::new(plugins::PluginResolver::new(environment.clone(), plugin_cache));

  let result = run_cli::run_cli(&args, &environment, &plugin_resolver).await;
  Ok(result?)
}
