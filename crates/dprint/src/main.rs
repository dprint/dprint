#[macro_use(err_obj)]
#[macro_use(err)]
extern crate dprint_core;
#[cfg(test)]
#[macro_use]
extern crate lazy_static;
#[macro_use]
mod environment;

use dprint_core::types::ErrBox;
use std::sync::Arc;
use environment::RealEnvironment;

mod cache;
mod cli;
mod configuration;
mod plugins;
mod utils;

#[tokio::main]
async fn main() -> Result<(), ErrBox> {
    match run().await {
        Ok(_) => {},
        Err(err) => {
            eprintln!("{}", err.to_string());
            std::process::exit(1);
        }
    }

    Ok(())
}

async fn run() -> Result<(), ErrBox> {
    #[cfg(windows)]
    colored::control::set_virtual_terminal(true).unwrap(); // the docs said this will always be Ok(())

    let stdin_reader = cli::RealStdInReader::new();
    let args = cli::parse_args(std::env::args().collect(), &stdin_reader)?;
    let environment = RealEnvironment::new(args.verbose, args.is_silent_output());
    let cache = Arc::new(cache::Cache::new(environment.clone())?);
    let plugin_cache = Arc::new(plugins::PluginCache::new(environment.clone())?);
    let plugin_pools = Arc::new(plugins::PluginPools::new(environment.clone()));
    let _plugins_dropper = plugins::PluginsDropper::new(plugin_pools.clone());
    let plugin_resolver = plugins::PluginResolver::new(environment.clone(), plugin_cache, plugin_pools.clone());

    cli::run_cli(args, &environment, &cache, &plugin_resolver, plugin_pools.clone()).await
}
