#[macro_use(err_obj)]
#[macro_use(err)]
extern crate dprint_core;
#[cfg(test)]
#[macro_use]
extern crate lazy_static;
#[macro_use]
mod environment;

use dprint_core::types::ErrBox;
use environment::RealEnvironment;
use std::sync::Arc;

mod cache;
mod cli;
mod configuration;
mod plugins;
mod utils;

#[cfg(test)]
mod test_helpers;

fn main() -> Result<(), ErrBox> {
  match run() {
    Ok(_) => {}
    Err(err) => {
      eprintln!("{}", err.to_string());
      std::process::exit(1);
    }
  }

  Ok(())
}

fn run() -> Result<(), ErrBox> {
  let stdin_reader = cli::RealStdInReader::new();
  let args = cli::parse_args(wild::args().collect(), &stdin_reader)?;
  let environment = RealEnvironment::new(args.verbose, args.is_silent_output())?;
  let cache = Arc::new(cache::Cache::new(environment.clone()));
  let plugin_cache = Arc::new(plugins::PluginCache::new(environment.clone()));
  let plugin_pools = Arc::new(plugins::PluginPools::new(environment.clone()));
  let _plugins_dropper = plugins::PluginsDropper::new(plugin_pools.clone());
  let plugin_resolver = plugins::PluginResolver::new(environment.clone(), plugin_cache, plugin_pools.clone());

  cli::run_cli(args, &environment, &cache, &plugin_resolver, plugin_pools.clone())
}
