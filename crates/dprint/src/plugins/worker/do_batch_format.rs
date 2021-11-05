use dprint_cli_core::types::ErrBox;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use crate::environment::Environment;
use crate::plugins::InitializedPlugin;
use crate::plugins::InitializedPluginPool;
use crate::plugins::PluginPools;
use crate::plugins::TakePluginResult;
use crate::utils::ErrorCountLogger;

use super::LongFormatCheckerThread;
use super::Worker;
use super::WorkerRegistry;

pub fn do_batch_format<TEnvironment: Environment, F>(
  environment: &TEnvironment,
  error_logger: &ErrorCountLogger<TEnvironment>,
  plugin_pools: &Arc<PluginPools<TEnvironment>>,
  file_paths_by_plugin: HashMap<String, Vec<PathBuf>>,
  action: F,
) -> Result<(), ErrBox>
where
  F: Fn(&InitializedPluginPool<TEnvironment>, &Path, &mut Box<dyn InitializedPlugin>) + Send + 'static + Clone,
{
  let registry = Arc::new(WorkerRegistry::new(plugin_pools.clone(), file_paths_by_plugin));

  // create a thread that will watch all the workers and report to the user when a file is taking a long time
  let long_format_checker_thread = LongFormatCheckerThread::new(environment, registry.clone());

  // spawn a thread for 1..n workers (exclude first)
  let thread_handles = registry
    .workers
    .iter()
    .skip(1)
    .map(|worker| {
      let worker = worker.clone();
      let error_logger = error_logger.clone();
      let action = action.clone();
      let registry = registry.clone();
      thread::spawn(move || run_thread(&error_logger, registry, &worker, action))
    })
    .collect::<Vec<_>>();

  // spawn the thread to check for files that take a long time to format
  long_format_checker_thread.spawn();

  // run the first worker on the current thread
  let first_worker = registry.workers.first().unwrap().clone();
  run_thread(error_logger, registry, &first_worker, action);

  // wait for the other threads to finish
  for handle in thread_handles {
    if let Err(_) = handle.join() {
      long_format_checker_thread.signal_exit();
      // todo: how to return error message?
      return err!("A panic occurred. You may want to run in verbose mode (--verbose) to help figure out where it failed then report this as a bug.",);
    }
  }

  long_format_checker_thread.signal_exit();

  return Ok(());
}

fn run_thread<TEnvironment: Environment, F>(
  error_logger: &ErrorCountLogger<TEnvironment>,
  registry: Arc<WorkerRegistry<TEnvironment>>,
  worker: &Worker<TEnvironment>,
  action: F,
) where
  F: Fn(&InitializedPluginPool<TEnvironment>, &Path, &mut Box<dyn InitializedPlugin>) + Send + 'static + Clone,
{
  let mut current_plugin: Option<(Box<dyn InitializedPlugin>, Arc<InitializedPluginPool<TEnvironment>>)> = None;
  loop {
    if let Err(err) = do_local_work(error_logger, &registry, &worker, action.clone(), current_plugin.take()) {
      error_logger.log_error(&err.to_string());
      return;
    }

    if let Some(stolen_work) = registry.steal_work(worker.id) {
      if let Some(plugin) = stolen_work.plugin {
        current_plugin = Some((plugin, stolen_work.work.pool.clone()));
      }
      worker.add_work(stolen_work.work);
    } else {
      return; // no more work left to steal
    }
  }
}

fn do_local_work<TEnvironment: Environment, F>(
  error_logger: &ErrorCountLogger<TEnvironment>,
  registry: &WorkerRegistry<TEnvironment>,
  worker: &Worker<TEnvironment>,
  action: F,
  current_plugin: Option<(Box<dyn InitializedPlugin>, Arc<InitializedPluginPool<TEnvironment>>)>,
) -> Result<(), ErrBox>
where
  F: Fn(&InitializedPluginPool<TEnvironment>, &Path, &mut Box<dyn InitializedPlugin>) + Send + 'static + Clone,
{
  let mut current_plugin = current_plugin;

  loop {
    let (pool, file_path) = if let Some(next_work) = worker.take_next_work() {
      next_work
    } else {
      // release the current plugin before exiting
      release_current_plugin(&mut current_plugin, registry, worker);
      return Ok(()); // finished the local work
    };

    // release the current plugin if it's changed
    if let Some((_, current_pool)) = current_plugin.as_ref() {
      if current_pool.name() != pool.name() {
        release_current_plugin(&mut current_plugin, registry, worker);
      }
    }

    // now ensure the current plugin is set if not
    if current_plugin.is_none() {
      match pool.take_or_create_checking_config_diagnostics(error_logger)? {
        TakePluginResult::Success(plugin) => {
          current_plugin = Some((plugin, pool));
        }
        TakePluginResult::HadDiagnostics => {
          // clear out all the work for the plugin on the current thread (other threads will figure this out on their own)
          worker.clear_work_for_current_plugin();
          continue;
        }
      }
    }

    // now do the work using it
    let plugin_and_pool = current_plugin.as_mut().unwrap();

    action(&plugin_and_pool.1, &file_path, &mut plugin_and_pool.0);
  }

  fn release_current_plugin<TEnvironment: Environment>(
    current_plugin: &mut Option<(Box<dyn InitializedPlugin>, Arc<InitializedPluginPool<TEnvironment>>)>,
    registry: &WorkerRegistry<TEnvironment>,
    worker: &Worker<TEnvironment>,
  ) {
    if let Some((current_plugin, pool)) = current_plugin.take() {
      pool.release(current_plugin);

      // if no other worker is working on this pool, then release the pool's resources
      registry.release_pool_if_no_work_in_registry(worker.id, pool.name());
    }
  }
}
