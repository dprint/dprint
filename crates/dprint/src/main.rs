use environment::RealEnvironment;

#[macro_use]
mod types;
mod environment;

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
    let args = cli::parse_args(std::env::args().collect())?;
    let environment = RealEnvironment::new(args.verbose);
    let plugin_resolver = plugins::wasm::WasmPluginResolver::new(&environment, &crate::plugins::wasm::compile);

    cli::run_cli(args, &environment, &plugin_resolver).await
}

