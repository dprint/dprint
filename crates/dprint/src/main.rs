#[cfg(test)]
#[macro_use]
extern crate lazy_static;
#[macro_use]
mod environment;

use anyhow::Result;
use environment::RealEnvironment;
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

fn main() -> Result<()> {
  match run() {
    Ok(_) => {}
    Err(err) => {
      eprintln!("{}", err.to_string());
      std::process::exit(1);
    }
  }

  Ok(())
}

fn run() -> Result<()> {
  let args = arg_parser::parse_args(wild::args().collect(), RealStdInReader)?;
  let environment = RealEnvironment::new(args.verbose, args.is_silent_output())?;
  let cache = Arc::new(cache::Cache::new(environment.clone()));
  let plugin_cache = Arc::new(plugins::PluginCache::new(environment.clone()));
  let plugin_pools = Arc::new(plugins::PluginPools::new(environment.clone()));
  let _plugins_dropper = plugins::PluginsDropper::new(plugin_pools.clone());
  let plugin_resolver = plugins::PluginResolver::new(environment.clone(), plugin_cache, plugin_pools.clone());

  run_cli::run_cli(&args, &environment, &cache, &plugin_resolver, plugin_pools)
}
