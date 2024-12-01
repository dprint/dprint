#![deny(clippy::print_stderr)]
#![deny(clippy::print_stdout)]
#![deny(clippy::unused_async)]

#[macro_use]
mod environment;

use anyhow::Result;
use dprint_core::plugins::process::setup_exit_process_panic_hook;
use environment::RealEnvironment;
use environment::RealEnvironmentOptions;
use run_cli::AppError;
use std::rc::Rc;
use utils::LogLevel;
use utils::RealStdInReader;

mod arg_parser;
mod commands;
mod configuration;
mod format;
mod incremental;
mod paths;
mod patterns;
mod plugins;
mod resolution;
mod run_cli;
mod utils;

#[cfg(test)]
mod test_helpers;

fn main() {
  setup_exit_process_panic_hook();
  let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
  rt.block_on(async move {
    match run().await {
      Ok(_) => {}
      Err((err, log_level)) => {
        if log_level != LogLevel::Silent {
          let result = format!("{:#}", err.inner);
          #[allow(clippy::print_stderr)]
          if !result.is_empty() {
            eprintln!("{}", result);
          }
        }
        std::process::exit(err.exit_code);
      }
    }
  });
}

async fn run() -> Result<(), (AppError, LogLevel)> {
  let args = arg_parser::parse_args(std::env::args().collect(), RealStdInReader).map_err(|err| (err.into(), LogLevel::Info))?;

  let environment = RealEnvironment::new(RealEnvironmentOptions {
    log_level: args.log_level,
    is_stdout_machine_readable: args.is_stdout_machine_readable(),
  })
  .map_err(|err| (err.into(), args.log_level))?;
  let plugin_cache = plugins::PluginCache::new(environment.clone());
  let plugin_resolver = Rc::new(plugins::PluginResolver::new(environment.clone(), plugin_cache));

  let result = run_cli::run_cli(&args, &environment, &plugin_resolver).await;
  plugin_resolver.clear_and_shutdown_initialized().await;
  result.map_err(|err| (err.into(), args.log_level))
}
