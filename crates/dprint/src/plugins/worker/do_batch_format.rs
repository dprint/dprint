use dprint_cli_core::types::ErrBox;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use crate::environment::Environment;
use crate::paths::PluginNames;
use crate::plugins::OptionalPluginAndPool;
use crate::plugins::PluginAndPoolMutRef;
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
  file_paths_by_plugins: HashMap<PluginNames, Vec<PathBuf>>,
  action: F,
) -> Result<(), ErrBox>
where
  F: Fn(Vec<PluginAndPoolMutRef<TEnvironment>>, &Path) + Send + 'static + Clone,
{
  let registry = Arc::new(WorkerRegistry::new(plugin_pools.clone(), file_paths_by_plugins));

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
  F: Fn(Vec<PluginAndPoolMutRef<TEnvironment>>, &Path) + Send + 'static + Clone,
{
  let mut current_plugins: Option<Vec<OptionalPluginAndPool<TEnvironment>>> = None;
  loop {
    if let Err(err) = do_local_work(error_logger, &registry, &worker, action.clone(), current_plugins.take()) {
      error_logger.log_error(&err.to_string());
      return;
    }

    if let Some(stolen_work) = registry.steal_work(worker.id) {
      if let Some(plugins) = stolen_work.plugins {
        current_plugins = Some(plugins);
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
  mut current_plugins: Option<Vec<OptionalPluginAndPool<TEnvironment>>>,
) -> Result<(), ErrBox>
where
  F: Fn(Vec<PluginAndPoolMutRef<TEnvironment>>, &Path) + Send + 'static + Clone,
{
  loop {
    let (pools, file_path) = if let Some(next_work) = worker.take_next_work() {
      next_work
    } else {
      // release the current plugins before exiting
      release_current_plugins(&mut current_plugins, registry, worker);
      return Ok(()); // finished the local work
    };

    // release the current plugin if it's changed
    if let Some(current_plugin_and_pools) = current_plugins.as_ref() {
      let has_changed = current_plugin_and_pools.len() != pools.len()
        || current_plugin_and_pools
          .iter()
          .map(|p| p.pool.name())
          .zip(pools.iter().map(|p| p.name()))
          .any(|(a, b)| a != b);
      if has_changed {
        release_current_plugins(&mut current_plugins, registry, worker);
      }
    }

    // now ensure the current plugin is set if not
    let current_plugins = if let Some(current_plugins) = current_plugins.as_mut() {
      current_plugins
    } else {
      current_plugins = Some(pools.iter().map(|pool| OptionalPluginAndPool::from_pool(pool.clone())).collect());
      current_plugins.as_mut().unwrap()
    };

    let mut had_diagnostics = false;
    let mut plugins_and_pools = Vec::with_capacity(current_plugins.len());
    for optional_plugin_and_pool in current_plugins.iter_mut() {
      if optional_plugin_and_pool.plugin.is_none() {
        match optional_plugin_and_pool.pool.take_or_create_checking_config_diagnostics(error_logger)? {
          TakePluginResult::Success(plugin) => {
            optional_plugin_and_pool.plugin = Some(plugin);
          }
          TakePluginResult::HadDiagnostics => {
            // clear out all the work for the plugin on the current thread (other threads will figure this out on their own)
            worker.clear_work_for_current_plugin();
            had_diagnostics = true;
            break;
          }
        }
      }
      plugins_and_pools.push(PluginAndPoolMutRef {
        plugin: optional_plugin_and_pool.plugin.as_mut().unwrap(),
        pool: &optional_plugin_and_pool.pool,
      })
    }
    if had_diagnostics {
      continue;
    }

    // now do the work using it
    action(plugins_and_pools, &file_path);
  }

  fn release_current_plugins<TEnvironment: Environment>(
    current_plugins: &mut Option<Vec<OptionalPluginAndPool<TEnvironment>>>,
    registry: &WorkerRegistry<TEnvironment>,
    worker: &Worker<TEnvironment>,
  ) {
    if let Some(plugin_and_pools) = current_plugins.take() {
      for plugin_and_pool in plugin_and_pools {
        if let Some(plugin) = plugin_and_pool.plugin {
          plugin_and_pool.pool.release(plugin);

          // if no other worker is working on this pool, then release the pool's resources
          registry.release_pool_if_no_work_in_registry(worker.id, plugin_and_pool.pool.name());
        }
      }
    }
  }
}
