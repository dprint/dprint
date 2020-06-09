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

    let args = cli::parse_args(std::env::args().collect())?;
    let environment = RealEnvironment::new(args.verbose);
    let cache = cache::Cache::new(&environment)?;
    let plugin_cache = plugins::PluginCache::new(&environment, &cache, &crate::plugins::wasm::compile);
    let plugin_resolver = plugins::wasm::WasmPluginResolver::new(&environment, &plugin_cache);

    cli::run_cli(args, &environment, &cache, &plugin_resolver).await
}

