use std::path::PathBuf;
use std::time::Instant;

use crate::environment::Environment;

use super::LocalPluginWork;
use super::PluginStealInfo;

pub enum LocalWorkStealKind {
  Immediate,
  Items(PluginStealInfo),
}

pub struct LocalWorkStealInfo {
  pub stealer_id: usize,
  pub kind: LocalWorkStealKind,
}

impl LocalWorkStealInfo {
  pub fn has_all_plugins_available(&self) -> bool {
    match &self.kind {
      LocalWorkStealKind::Items(items) => items.has_all_plugins_available,
      _ => false,
    }
  }
}

#[derive(Clone)]
pub struct FormattingFilePathInfo {
  pub start_time: Instant,
  pub file_path: PathBuf,
}

pub struct LocalWork<TEnvironment: Environment> {
  pub work_by_plugin: Vec<LocalPluginWork<TEnvironment>>,
  pub stealer_id: usize,
  /// The file path currently being formatted. This is used to tell when a worker
  /// is taking too much time.
  current_formatting_file_path: Option<FormattingFilePathInfo>,
}

impl<TEnvironment: Environment> LocalWork<TEnvironment> {
  pub fn new(work_by_plugin: Vec<LocalPluginWork<TEnvironment>>) -> Self {
    LocalWork {
      work_by_plugin,
      stealer_id: 0,
      current_formatting_file_path: None,
    }
  }

  pub fn calculate_worthwhile_steal_time(&self) -> Option<LocalWorkStealInfo> {
    if self.work_by_plugin.len() > 1 {
      Some(LocalWorkStealInfo {
        stealer_id: self.stealer_id,
        kind: LocalWorkStealKind::Immediate,
      })
    } else {
      self
        .work_by_plugin
        .get(0)
        .and_then(|plugin_work| plugin_work.calculate_worthwhile_steal_time())
        .map(|plugin_info| LocalWorkStealInfo {
          stealer_id: self.stealer_id,
          kind: LocalWorkStealKind::Items(plugin_info),
        })
    }
  }

  pub fn get_current_formatting_file_path_info(&self) -> Option<FormattingFilePathInfo> {
    self.current_formatting_file_path.clone()
  }

  pub fn set_current_formatting_file_path(&mut self, file_path: PathBuf) {
    self.current_formatting_file_path = Some(FormattingFilePathInfo {
      start_time: Instant::now(),
      file_path,
    });
  }

  pub fn clear_current_formatting_file_path(&mut self) {
    self.current_formatting_file_path.take();
  }
}
