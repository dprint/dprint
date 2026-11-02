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
mod cache;
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
  // wasm plugins run on blocking threads and execute wasm on the native stack,
  // so give blocking threads a stack large enough (see WASM_PLUGIN_THREAD_STACK_SIZE).
  let rt = tokio::runtime::Builder::new_current_thread()
    .enable_io()
    .enable_time()
    .thread_stack_size(crate::plugins::WASM_PLUGIN_THREAD_STACK_SIZE)
    .build()
    .unwrap();
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
  let plugin_resolver = Rc::new(plugins::PluginResolver::new(
    environment.clone(),
    plugin_cache,
    plugins::PluginResolverOptions {
      reload_plugins: args.reload.plugins,
    },
  ));

  let result = run_cli::run_cli(&args, &environment, &plugin_resolver).await;
  plugin_resolver.clear_and_shutdown_initialized().await;
  if result.is_ok() && matches!(args.sub_command, arg_parser::SubCommand::EditorService(_)) {
    // Exit the process instead of returning. The editor service reads stdin on a
    // blocking task that stays parked in a read while the editor keeps the pipe
    // open and dropping the tokio runtime waits for that task, so returning would
    // keep the process alive until the editor closes stdin.
    std::process::exit(0);
  }
  result.map_err(|err| (err.into(), args.log_level))
}
