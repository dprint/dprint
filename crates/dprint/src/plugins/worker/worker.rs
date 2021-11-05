use parking_lot::RwLock;
use std::path::PathBuf;
use std::sync::Arc;

use crate::environment::Environment;
use crate::plugins::InitializedPlugin;
use crate::plugins::InitializedPluginPool;

use super::FormattingFilePathInfo;
use super::LocalPluginWork;
use super::LocalWork;
use super::LocalWorkStealInfo;

pub struct StealResult<TEnvironment: Environment> {
  pub plugin: Option<Box<dyn InitializedPlugin>>,
  pub work: LocalPluginWork<TEnvironment>,
}

pub struct Worker<TEnvironment: Environment> {
  pub id: usize,
  local_work: RwLock<LocalWork<TEnvironment>>,
}

impl<TEnvironment: Environment> Worker<TEnvironment> {
  pub fn new(id: usize, work_by_plugin: Vec<LocalPluginWork<TEnvironment>>) -> Self {
    Worker {
      id,
      local_work: RwLock::new(LocalWork::new(work_by_plugin)),
    }
  }

  pub fn get_current_formatting_file_path_info(&self) -> Option<FormattingFilePathInfo> {
    self.local_work.read().get_current_formatting_file_path_info()
  }

  pub fn has_pool(&self, pool_name: &str) -> bool {
    let local_work = self.local_work.read();
    for work in local_work.work_by_plugin.iter() {
      if work.pool.name() == pool_name {
        return true;
      }
    }
    false
  }

  pub fn calculate_worthwhile_steal_time(&self) -> Option<LocalWorkStealInfo> {
    self.local_work.read().calculate_worthwhile_steal_time()
  }

  pub fn try_steal(&self, steal_info: LocalWorkStealInfo) -> Option<StealResult<TEnvironment>> {
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
      if plugin_work.work_items_len() > 1 {
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
          work: plugin_work.split(),
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

  pub fn add_work(&self, work: LocalPluginWork<TEnvironment>) {
    self.local_work.write().work_by_plugin.push(work);
  }

  pub fn take_next_work(&self) -> Option<(Arc<InitializedPluginPool<TEnvironment>>, PathBuf)> {
    let mut local_work = self.local_work.write();
    if let Some(work_by_plugin) = local_work.work_by_plugin.get_mut(0) {
      let pool = work_by_plugin.pool.clone();
      let file_path = work_by_plugin.take_next_work_item();
      if work_by_plugin.work_items_len() == 0 {
        local_work.work_by_plugin.remove(0);
      }
      local_work.set_current_formatting_file_path(file_path.clone());
      Some((pool, file_path))
    } else {
      local_work.clear_current_formatting_file_path();
      None
    }
  }

  pub fn clear_work_for_current_plugin(&self) {
    let mut local_work = self.local_work.write();
    if !local_work.work_by_plugin.is_empty() {
      local_work.work_by_plugin.remove(0);
    }
  }
}
