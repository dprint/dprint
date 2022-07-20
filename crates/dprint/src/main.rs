#[cfg(test)]
#[macro_use]
extern crate lazy_static;
#[macro_use]
mod environment;

use anyhow::Result;
use dprint_core::plugins::process::setup_exit_process_panic_hook;
use environment::RealEnvironment;
use environment::RealEnvironmentOptions;
use std::sync::Arc;
use utils::RealStdInReader;

mod arg_parser;
mod cache;
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
        eprintln!("{:#}", err);
        std::process::exit(1);
      }
    }
  });
}

async fn run(runtime_handle: tokio::runtime::Handle) -> Result<()> {
  let args = arg_parser::parse_args(std::env::args().collect(), RealStdInReader)?;
  let environment = RealEnvironment::new(RealEnvironmentOptions {
    is_verbose: args.verbose,
    is_stdout_machine_readable: args.is_stdout_machine_readable(),
    runtime_handle: Arc::new(runtime_handle),
  })?;
  let cache = Arc::new(cache::Cache::new(environment.clone()));
  let plugin_cache = Arc::new(plugins::PluginCache::new(environment.clone()));
  let plugin_pools = Arc::new(plugins::PluginsCollection::new(environment.clone()));
  let plugin_resolver = plugins::PluginResolver::new(environment.clone(), plugin_cache, plugin_pools.clone());

  let result = run_cli::run_cli(&args, &environment, &cache, &plugin_resolver, plugin_pools.clone()).await;
  plugin_pools.drop_and_shutdown_initialized().await;
  result
}
