use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::environment::Environment;
use crate::plugins::PluginPools;

use super::{LocalPluginWork, LocalWorkStealInfo, LocalWorkStealKind, StealResult, Worker};

pub struct WorkerRegistry<TEnvironment: Environment> {
  plugin_pools: Arc<PluginPools<TEnvironment>>,
  pub workers: Vec<Arc<Worker<TEnvironment>>>,
}

impl<TEnvironment: Environment> WorkerRegistry<TEnvironment> {
  pub fn new(plugin_pools: Arc<PluginPools<TEnvironment>>, file_paths_by_plugin: HashMap<String, Vec<PathBuf>>) -> Self {
    let workers = get_workers(&plugin_pools, file_paths_by_plugin);
    return WorkerRegistry { plugin_pools, workers };

    fn get_workers<TEnvironment: Environment>(
      plugin_pools: &PluginPools<TEnvironment>,
      file_paths_by_plugin: HashMap<String, Vec<PathBuf>>,
    ) -> Vec<Arc<Worker<TEnvironment>>> {
      let number_threads = std::cmp::max(1, num_cpus::get()); // use logical cores (same as Rayon)
      let mut workers = Vec::with_capacity(number_threads);

      // initially divide work by plugins
      let mut item_stacks = Vec::with_capacity(number_threads);
      for (i, (plugin_name, file_paths)) in file_paths_by_plugin.into_iter().enumerate() {
        let i = i % number_threads;
        if item_stacks.get(i).is_none() {
          item_stacks.push(Vec::new());
        }
        item_stacks
          .get_mut(i)
          .unwrap()
          .push(LocalPluginWork::new(plugin_pools.get_pool(&plugin_name).unwrap(), file_paths));
      }

      // create the workers
      for i in 0..number_threads {
        workers.push(Arc::new(Worker::new(i, item_stacks.pop().unwrap_or(Vec::new()))));
      }

      debug_assert!(item_stacks.is_empty());

      workers
    }
  }

  pub fn release_pool_if_no_work_in_registry(&self, asking_worker_id: usize, pool_name: &str) {
    // if no other worker is working on this pool, then release the pool's resources
    if !self.any_worker_has_pool(asking_worker_id, pool_name) {
      self.plugin_pools.release(pool_name)
    }
  }

  /// checks if other workers have work for the specified pool
  fn any_worker_has_pool(&self, asking_worker_id: usize, pool_name: &str) -> bool {
    for worker in self.workers.iter() {
      // skip checking the current worker
      if worker.id == asking_worker_id {
        continue;
      }
      if worker.has_pool(pool_name) {
        return true;
      }
    }

    false
  }

  pub fn steal_work(&self, asking_worker_id: usize) -> Option<StealResult<TEnvironment>> {
    // evaluate which worker might be best to steal from
    loop {
      // first figure out what to steal
      if let Some((steal_info, worker)) = self.find_work_to_steal(asking_worker_id) {
        // now attempt to steal... if we don't succeed, try again
        if let Some(steal_result) = worker.try_steal(steal_info) {
          return Some(steal_result);
        }
      } else {
        // there is no more work to do
        return None;
      }
    }
  }

  fn find_work_to_steal(&self, asking_worker_id: usize) -> Option<(LocalWorkStealInfo, Arc<Worker<TEnvironment>>)> {
    // evaluate which worker might be best to steal from
    let mut best_match: Option<(LocalWorkStealInfo, &Arc<Worker<TEnvironment>>)> = None;
    for worker in self.workers.iter() {
      // current worker won't have anything to steal
      if worker.id == asking_worker_id {
        continue;
      }

      if let Some(steal_info) = worker.calculate_worthwhile_steal_time() {
        match &steal_info.kind {
          LocalWorkStealKind::Immediate => {
            // steal from this one right away
            return Some((steal_info, worker.to_owned()));
          }
          LocalWorkStealKind::Items(plugin_info) => {
            if let Some(best_match) = best_match.as_mut() {
              if let LocalWorkStealKind::Items(best_match_plugin_info) = &best_match.0.kind {
                if best_match_plugin_info.has_plugin_available != plugin_info.has_plugin_available {
                  // always first consider work that has a plugin available
                  if plugin_info.has_plugin_available {
                    *best_match = (steal_info, &worker);
                  }
                } else if plugin_info.steal_time > best_match_plugin_info.steal_time {
                  *best_match = (steal_info, &worker);
                }
              } else {
                panic!("For some reason the best match was immediate.");
              }
            } else {
              best_match = Some((steal_info, &worker));
            }
          }
        }
      }
    }
    best_match.map(|(steal_info, worker)| (steal_info, worker.to_owned()))
  }
}
