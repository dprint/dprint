use std::collections::HashSet;
use std::path::PathBuf;
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

/// Re-entrant LaxSingleProcessFsFlag at a path source. This attempts to
/// prevent multiple processes from modifying the cache at the same time.
pub struct CacheFsLockPool<TEnvironment: Environment> {
  environment: TEnvironment,
  locks: Arc<Mutex<HashSet<u64>>>,
  cache_dir: PathBuf,
}

impl<TEnvironment: Environment> CacheFsLockPool<TEnvironment> {
  pub fn new(environment: TEnvironment) -> Self {
    let cache_dir = environment.get_cache_dir().join("locks");
    let _ = environment.mk_dir_all(&cache_dir);
    Self::new_with_cache_dir(environment, cache_dir)
  }

  fn new_with_cache_dir(environment: TEnvironment, cache_dir: PathBuf) -> Self {
    Self {
      environment,
      cache_dir,
      locks: Arc::new(Mutex::new(HashSet::new())),
    }
  }

  /// Objects a file system lock for the provided path source. Locks are re-entrant
  /// for the current process, but WARNING that they currently don't handle the guards
  /// being dropped out of order. For the current code consuming this, it's ok.
  pub async fn lock(&self, path_source: &PathSource) -> CacheFsLockGuard<TEnvironment> {
    let id = get_bytes_hash(path_source.display().as_bytes());
    // ensure this process only sets the lock once for this id
    if self.locks.lock().insert(id) {
      let plugin_sync_id = format!(".{}.lock", id);
      let long_wait_message = format!("Waiting for file lock for '{}'...", path_source.display());
      let fs_flag = LaxSingleProcessFsFlag::lock(&self.environment, self.cache_dir.join(&plugin_sync_id), &long_wait_message).await;
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

#[cfg(test)]
mod test {
  use std::sync::Arc;

  use futures::FutureExt;
  use tempfile::TempDir;
  use url::Url;

  use crate::environment::RealEnvironment;

  use super::*;

  #[test]
  fn pool_re_entrant_same_process() {
    RealEnvironment::run_test_with_real_env(|env| {
      async move {
        let temp_dir = TempDir::new().unwrap();
        let pool = Arc::new(CacheFsLockPool::new_with_cache_dir(env.clone(), temp_dir.path().to_path_buf()));
        let url = "https://dprint.dev/test/test.json";
        let source = PathSource::new_remote(Url::parse(&url).unwrap());
        // just ensure this is re-entrant
        let flag1 = pool.lock(&source).await;
        let flag2 = pool.lock(&source).await;
        assert!(flag1.0.is_some());
        assert!(flag2.0.is_none());
        drop(flag2);
        drop(flag1);

        // ensure it creates one the second time
        let flag1 = pool.lock(&source).await;
        let flag2 = pool.lock(&source).await;
        assert!(flag1.0.is_some());
        assert!(flag2.0.is_none());
      }
      .boxed()
    })
  }
}
