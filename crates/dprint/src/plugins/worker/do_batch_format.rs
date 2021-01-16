use dprint_cli_core::types::ErrBox;
use std::thread;
use std::collections::HashMap;
use parking_lot::RwLock;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::environment::Environment;
use crate::plugins::{InitializedPlugin, InitializedPluginPool, PluginPools, TakePluginResult};
use crate::utils::ErrorCountLogger;

use super::Deque;

pub fn do_batch_format<TEnvironment: Environment, F>(
    error_logger: &ErrorCountLogger<TEnvironment>,
    plugin_pools: &Arc<PluginPools<TEnvironment>>,
    file_paths_by_plugin: HashMap<String, Vec<PathBuf>>,
    action: F
) -> Result<(), ErrBox> where F: Fn(&InitializedPluginPool<TEnvironment>, &Path, &mut Box<dyn InitializedPlugin>) + Send + 'static + Clone {
    let registry: Arc<WorkerRegistry<TEnvironment>> = Arc::new(WorkerRegistry {
        plugin_pools: plugin_pools.clone(),
        workers: RwLock::new(Vec::new()),
    });
    let mut workers = add_workers_to_registry(plugin_pools, &registry, file_paths_by_plugin);

    // spawn a thread for n-1 workers
    let last_worker = workers.pop().unwrap();
    let thread_handles = workers.into_iter().map(|worker| {
        let error_logger = error_logger.clone();
        let action = action.clone();
        thread::spawn(move || {
            run_thread(&error_logger, &worker, action)
        })
    }).collect::<Vec<_>>();

    // run the last worker on the current thread
    run_thread(error_logger, &last_worker, action);

    // wait for the other threads to finish
    for handle in thread_handles {
        if let Err(_) = handle.join() {
            // todo: how to return error message?
            return err!(
                "A panic occurred. You may want to run in verbose mode (--verbose) to help figure out where it failed then report this as a bug.",
            );
        }
    }

    // allow the registry to be dropped by clearing the workers (since there's a circular reference)
    registry.clear_workers();

    return Ok(());

    fn add_workers_to_registry<TEnvironment: Environment>(
        plugin_pools: &PluginPools<TEnvironment>,
        registry: &Arc<WorkerRegistry<TEnvironment>>,
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
            item_stacks.get_mut(i).unwrap().push(LocalPluginWork {
                pool: plugin_pools.get_pool(&plugin_name).unwrap(),
                items: Deque::new(file_paths),
            });
        }

        // create the workers
        for i in 0..number_threads {
            workers.push(registry.add_worker(Worker {
                id: i,
                registry: registry.clone(),
                local_work: RwLock::new(LocalWork {
                    work_by_plugin: item_stacks.pop().unwrap_or(Vec::new()),
                    stealer_id: 0
                })
            }));
        }

        debug_assert!(item_stacks.is_empty());

        workers
    }
}

fn run_thread<TEnvironment: Environment, F>(
    error_logger: &ErrorCountLogger<TEnvironment>,
    worker: &Worker<TEnvironment>,
    action: F,
) where F: Fn(&InitializedPluginPool<TEnvironment>, &Path, &mut Box<dyn InitializedPlugin>) + Send + 'static + Clone {
    let mut current_plugin: Option<(Box<dyn InitializedPlugin>, Arc<InitializedPluginPool<TEnvironment>>)> = None;
    loop {
        if let Err(err) = do_local_work(error_logger, &worker, action.clone(), current_plugin.take()) {
            error_logger.log_error(&err.to_string());
            return;
        }

        if let Some(stolen_work) = worker.registry.steal_work(worker.id) {
            if let Some(plugin) = stolen_work.plugin {
                current_plugin = Some((plugin, stolen_work.work.pool.clone()));
            }
            worker.local_work.write().work_by_plugin.push(stolen_work.work);
        } else {
            return; // no more work left to steal
        }
    }
}

fn do_local_work<TEnvironment: Environment, F>(
    error_logger: &ErrorCountLogger<TEnvironment>,
    worker: &Worker<TEnvironment>,
    action: F,
    current_plugin: Option<(Box<dyn InitializedPlugin>, Arc<InitializedPluginPool<TEnvironment>>)>,
) -> Result<(), ErrBox> where F: Fn(&InitializedPluginPool<TEnvironment>, &Path, &mut Box<dyn InitializedPlugin>) + Send + 'static + Clone {
    let mut current_plugin = current_plugin;

    loop {
        let mut should_release = false;
        let (pool, file_path) = {
            let mut local_work = worker.local_work.write();
            if let Some(work_by_plugin) = local_work.work_by_plugin.get_mut(0) {
                let pool = work_by_plugin.pool.clone();
                let file_path = work_by_plugin.items.dequeue().unwrap().to_owned();
                if work_by_plugin.items.len() == 0 {
                    local_work.work_by_plugin.remove(0);
                    should_release = true;
                }
                (pool, file_path)
            } else {
                return Ok(()); // finished the local work
            }
        };

        // release the current plugin if it's changed
        if let Some((_, current_pool)) = current_plugin.as_ref() {
            if current_pool.name() != pool.name() {
                if let Some((current_plugin, pool)) = current_plugin.take() {
                    pool.release(current_plugin);
                }
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
                    let mut local_work = worker.local_work.write();
                    if !local_work.work_by_plugin.is_empty() {
                        local_work.work_by_plugin.remove(0);
                    }
                    continue;
                }
            }
        }

        // now do the work using it
        let plugin_and_pool = current_plugin.as_mut().unwrap();

        action(&plugin_and_pool.1, &file_path, &mut plugin_and_pool.0);

        // are we all done local work for this plugin?
        if should_release {
            if let Some((current_plugin, pool)) = current_plugin.take() {
                pool.release(current_plugin);

                // if no other worker is working on this pool, then release the pool's resources
                worker.registry.release_pool_if_no_work_in_registry(worker.id, pool.name());
            }
        }
    }
}

struct PluginStealInfo {
    has_plugin_available: bool,
    steal_time: u64,
}

struct LocalPluginWork<TEnvironment: Environment> {
    pool: Arc<InitializedPluginPool<TEnvironment>>,
    items: Deque<PathBuf>,
}

impl<TEnvironment: Environment> LocalPluginWork<TEnvironment> {
    pub fn split(&mut self) -> LocalPluginWork<TEnvironment> {
        LocalPluginWork {
            pool: self.pool.clone(),
            items: self.items.split(),
        }
    }

    pub fn calculate_worthwhile_steal_time(&self) -> Option<PluginStealInfo> {
        let remaining_len = self.items.len() as u64;
        if remaining_len <= 1 {
            return None; // don't steal, not worth it
        }
        let time_snapshot = self.pool.get_time_snapshot();
        let actual_startup_time = if time_snapshot.has_plugin_available { 0 } else { time_snapshot.startup_time };
        let steal_time = (remaining_len / 2) * time_snapshot.average_format_time + actual_startup_time;
        let remaining_time = remaining_len * time_snapshot.average_format_time;
        if steal_time + time_snapshot.average_format_time * 2 > remaining_time {
            None // don't steal, not worth it
        } else {
            Some(PluginStealInfo {
                has_plugin_available: time_snapshot.has_plugin_available,
                steal_time,
            })
        }
    }
}

enum LocalWorkStealKind {
    Immediate,
    Items(PluginStealInfo),
}

struct LocalWorkStealInfo {
    stealer_id: usize,
    kind: LocalWorkStealKind,
}

impl LocalWorkStealInfo {
    pub fn has_plugin_available(&self) -> bool {
        match &self.kind {
            LocalWorkStealKind::Items(items) => items.has_plugin_available,
            _ => false,
        }
    }
}

struct LocalWork<TEnvironment: Environment> {
    work_by_plugin: Vec<LocalPluginWork<TEnvironment>>,
    stealer_id: usize,
}

impl<TEnvironment: Environment> LocalWork<TEnvironment> {
    pub fn calculate_worthwhile_steal_time(&self) -> Option<LocalWorkStealInfo> {
        if self.work_by_plugin.len() > 1 {
            Some(LocalWorkStealInfo {
                stealer_id: self.stealer_id,
                kind: LocalWorkStealKind::Immediate
            })
        } else {
            self.work_by_plugin.get(0)
                .map(|plugin_work| plugin_work.calculate_worthwhile_steal_time())
                .flatten()
                .map(|plugin_info| LocalWorkStealInfo {
                    stealer_id: self.stealer_id,
                    kind: LocalWorkStealKind::Items(plugin_info),
                })
        }
    }
}

struct StealResult<TEnvironment: Environment> {
    plugin: Option<Box<dyn InitializedPlugin>>,
    work: LocalPluginWork<TEnvironment>,
}

struct Worker<TEnvironment: Environment> {
    id: usize,
    registry: Arc<WorkerRegistry<TEnvironment>>,
    local_work: RwLock<LocalWork<TEnvironment>>,
}

impl<TEnvironment: Environment> Worker<TEnvironment> {
    fn calculate_worthwhile_steal_time(&self) -> Option<LocalWorkStealInfo> {
        self.local_work.read().calculate_worthwhile_steal_time()
    }

    fn try_steal(&self, steal_info: LocalWorkStealInfo) -> Option<StealResult<TEnvironment>> {
        let mut local_work = self.local_work.write();
        if local_work.stealer_id != steal_info.stealer_id {
            return None; // someone stole before us
        }

        if local_work.work_by_plugin.len() > 1 {
            // steal immediately
            let steal_result = StealResult {
                plugin: None,
                work: local_work.work_by_plugin.pop().unwrap(),
            };

            // Increment the stealer id to force another thread to re-evaluate who to steal from
            local_work.stealer_id += 1;

            Some(steal_result)
        } else if let Some(plugin_work) = local_work.work_by_plugin.get_mut(0) {
            if plugin_work.items.len() > 1 {
                let plugin = if steal_info.has_plugin_available() {
                    match plugin_work.pool.take_if_available() {
                        Some(plugin) => Some(plugin),
                        None => return None, // we did the steal evaluation based on the plugin being available and that's no longer the case
                    }
                } else {
                    None
                };
                let steal_result = StealResult {
                    plugin,
                    work: plugin_work.split()
                };

                // Increment the stealer id to force another thread to re-evaluate who to steal from
                local_work.stealer_id += 1;

                Some(steal_result)
            } else {
                None
            }
        } else {
            None
        }

    }
}

struct WorkerRegistry<TEnvironment: Environment> {
    plugin_pools: Arc<PluginPools<TEnvironment>>,
    // todo: don't use any locking on this as once it's set it doesn't change
    workers: RwLock<Vec<Arc<Worker<TEnvironment>>>>,
}

impl<TEnvironment: Environment> WorkerRegistry<TEnvironment> {
    fn add_worker(&self, worker: Worker<TEnvironment>) -> Arc<Worker<TEnvironment>> {
        let worker = Arc::new(worker);
        self.workers.write().push(worker.clone());
        worker
    }

    fn clear_workers(&self) {
        self.workers.write().clear();
    }

    pub fn release_pool_if_no_work_in_registry(&self, asking_worker_id: usize, pool_name: &str) {
        // if no other worker is working on this pool, then release the pool's resources
        if !self.any_worker_has_pool(asking_worker_id, pool_name) {
            self.plugin_pools.release(pool_name)
        }
    }

    /// checks if other workers have work for the specified pool
    fn any_worker_has_pool(&self, asking_worker_id: usize, pool_name: &str) -> bool {
        for worker in self.workers.read().iter() {
            // skip checking the current worker
            if worker.id == asking_worker_id {
                continue;
            }
            let local_work = worker.local_work.read();
            for work in local_work.work_by_plugin.iter() {
                if work.pool.name() == pool_name {
                    return true;
                }
            }
        }

        false
    }

    fn steal_work(&self, asking_worker_id: usize) -> Option<StealResult<TEnvironment>> {
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
        let workers = self.workers.read();
        let mut best_match: Option<(LocalWorkStealInfo, &Arc<Worker<TEnvironment>>)> = None;
        for worker in workers.iter() {
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
