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

#[tokio::main]
async fn main() -> Result<()> {
  setup_exit_process_panic_hook();
  match run().await {
    Ok(_) => {}
    Err(err) => {
      eprintln!("{}", err);
      std::process::exit(1);
    }
  }

  Ok(())
}

async fn run() -> Result<()> {
  let args = arg_parser::parse_args(wild::args().collect(), RealStdInReader)?;
  let environment = RealEnvironment::new(&RealEnvironmentOptions {
    is_verbose: args.verbose,
    is_stdout_machine_readable: args.is_stdout_machine_readable(),
  })?;
  let cache = Arc::new(cache::Cache::new(environment.clone()));
  let plugin_cache = Arc::new(plugins::PluginCache::new(environment.clone()));
  let plugin_pools = Arc::new(plugins::PluginsCollection::new(environment.clone()));
  let _plugins_dropper = plugins::PluginsDropper::new(plugin_pools.clone());
  let plugin_resolver = plugins::PluginResolver::new(environment.clone(), plugin_cache, plugin_pools.clone());

  run_cli::run_cli(&args, &environment, &cache, &plugin_resolver, plugin_pools).await
}
