use std::collections::HashSet;
use std::sync::Arc;

use parking_lot::Mutex;

use crate::environment::Environment;
use crate::utils::get_bytes_hash;
use crate::utils::LaxSingleProcessFsFlag;
use crate::utils::PathSource;

struct CacheFsLockGuardInner<TEnvironment: Environment> {
  id: u64,
  locks: Arc<Mutex<HashSet<u64>>>,
  // keep this alive for the duration of the guard
  _fs_flag: LaxSingleProcessFsFlag<TEnvironment>,
}

impl<TEnvironment: Environment> Drop for CacheFsLockGuardInner<TEnvironment> {
  fn drop(&mut self) {
    // allow this process to set the lock again
    self.locks.lock().remove(&self.id);
  }
}

pub struct CacheFsLockGuard<TEnvironment: Environment>(Option<CacheFsLockGuardInner<TEnvironment>>);

/// Re-entrant LaxSingleProcessFsFlag at a path source.
pub struct CacheFsLockPool<TEnvironment: Environment> {
  environment: TEnvironment,
  locks: Arc<Mutex<HashSet<u64>>>,
}

impl<TEnvironment: Environment> CacheFsLockPool<TEnvironment> {
  pub fn new(environment: TEnvironment) -> Self {
    Self {
      environment,
      locks: Arc::new(Mutex::new(HashSet::new())),
    }
  }

  pub async fn lock(&self, path_source: &PathSource) -> CacheFsLockGuard<TEnvironment> {
    let id = get_bytes_hash(path_source.display().as_bytes());
    // ensure this process only sets the lock once for this id
    if self.locks.lock().insert(id) {
      let plugin_sync_id = format!(".{}.lock", id);
      let long_wait_message = format!("Waiting for file lock for '{}'...", path_source.display());
      let plugins_dir = self.environment.get_cache_dir();
      let fs_flag = LaxSingleProcessFsFlag::lock(&self.environment, plugins_dir.join(&plugin_sync_id), &long_wait_message).await;
      CacheFsLockGuard(Some(CacheFsLockGuardInner {
        id,
        locks: self.locks.clone(),
        _fs_flag: fs_flag,
      }))
    } else {
      CacheFsLockGuard(None)
    }
  }
}
