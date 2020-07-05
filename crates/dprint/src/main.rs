use std::sync::Arc;
use environment::RealEnvironment;

#[macro_use]
mod types;
#[macro_use]
mod environment;

mod cache;
mod cli;
mod configuration;
mod plugins;
mod utils;

#[cfg(test)]
#[macro_use]
extern crate lazy_static;

#[tokio::main]
async fn main() -> Result<(), types::ErrBox> {
    match run().await {
        Ok(_) => {},
        Err(err) => {
            eprintln!("{}", err.to_string());
            std::process::exit(1);
        }
    }

    Ok(())
}

async fn run() -> Result<(), types::ErrBox> {
    #[cfg(windows)]
    colored::control::set_virtual_terminal(true).unwrap(); // the docs said this will always be ok

    let stdin_reader = cli::RealStdInReader::new();
    let args = cli::parse_args(std::env::args().collect(), &stdin_reader)?;
    let environment = RealEnvironment::new(args.verbose, args.is_silent_output());
    let cache = cache::Cache::new(&environment)?;
    let plugin_cache = plugins::PluginCache::new(&environment, &cache, &crate::plugins::wasm::compile);
    let plugin_pools = Arc::new(plugins::PluginPools::new(environment.clone()));
    let import_object = plugins::wasm::PoolPluginImportObject::new(plugin_pools.clone());
    let plugin_resolver = plugins::wasm::WasmPluginResolver::new(&environment, &plugin_cache, &import_object);

    cli::run_cli(args, &environment, &cache, &plugin_resolver, plugin_pools).await
}

