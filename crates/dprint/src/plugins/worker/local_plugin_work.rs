use std::path::PathBuf;
use std::sync::Arc;

use crate::environment::Environment;
use crate::plugins::InitializedPluginPool;

use super::Deque;

pub struct PluginStealInfo {
  pub has_all_plugins_available: bool,
  pub steal_time: u64,
}

pub struct LocalPluginWork<TEnvironment: Environment> {
  // there might be multiple plugins to format a file with
  pub pools: Arc<Vec<Arc<InitializedPluginPool<TEnvironment>>>>,
  items: Deque<PathBuf>,
}

impl<TEnvironment: Environment> LocalPluginWork<TEnvironment> {
  pub fn new(pools: Arc<Vec<Arc<InitializedPluginPool<TEnvironment>>>>, file_paths: Vec<PathBuf>) -> Self {
    LocalPluginWork {
      pools,
      items: Deque::new(file_paths),
    }
  }

  pub fn work_items_len(&self) -> usize {
    self.items.len()
  }

  pub fn take_next_work_item(&mut self) -> PathBuf {
    self.items.dequeue().unwrap().to_owned()
  }

  pub fn split(&mut self) -> LocalPluginWork<TEnvironment> {
    LocalPluginWork {
      pools: self.pools.clone(),
      items: self.items.split(),
    }
  }

  pub fn calculate_worthwhile_steal_time(&self) -> Option<PluginStealInfo> {
    let remaining_len = self.items.len() as u64;
    if remaining_len <= 1 {
      return None; // don't steal, not worth it
    }

    let mut has_all_plugins_available = true;
    let mut total_steal_time = 0;
    for pool in self.pools.iter() {
      let time_snapshot = pool.get_time_snapshot();
      let actual_startup_time = if time_snapshot.has_plugin_available { 0 } else { time_snapshot.startup_time };
      let steal_time = (remaining_len / 2) * time_snapshot.average_format_time + actual_startup_time;
      let remaining_time = remaining_len * time_snapshot.average_format_time;
      if steal_time + time_snapshot.average_format_time * 2 > remaining_time {
        return None; // don't steal, not worth it
      }
      total_steal_time += steal_time;

      if !time_snapshot.has_plugin_available {
        has_all_plugins_available = false;
      }
    }

    Some(PluginStealInfo {
      has_all_plugins_available,
      steal_time: total_steal_time,
    })
  }
}
