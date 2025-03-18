// Copyright 2018-2023 the Deno authors. All rights reserved. MIT license.

// This module is lifted and adapted from the Deno codebase
// https://github.com/denoland/deno/blob/17ddf2f97c58db0b6825809a8bc325f0bda65b1b/cli/util/fs.rs#L471

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::environment::Environment;

/// A file system based flag that will attempt to synchronize multiple
/// processes so they go one after the other. In scenarios where
/// synchronization cannot be achieved, it will allow the current process
/// to proceed.
///
/// This should only be used in places where it's ideal for multiple
/// processes to not update something on the file system at the same time,
/// but it's not that big of a deal.
pub struct LaxSingleProcessFsFlag<TEnvironment: Environment>(Option<LaxSingleProcessFsFlagInner<TEnvironment>>);

impl<TEnvironment: Environment> LaxSingleProcessFsFlag<TEnvironment> {
  pub async fn lock(environment: &TEnvironment, file_path: PathBuf, long_wait_message: &str) -> Self {
    // don't bother making this work for a test environment
    if !environment.is_real() {
      return Self(None);
    }

    log_debug!(environment, "Acquiring file lock at {}", file_path.display());
    use fs3::FileExt;
    let last_updated_path = file_path.with_extension("lock.poll");
    let start_instant = std::time::Instant::now();
    let open_result = std::fs::OpenOptions::new().read(true).write(true).truncate(true).create(true).open(&file_path);

    match open_result {
      Ok(fs_file) => {
        let mut pb_update_guard = None;
        let mut error_count = 0;
        while error_count < 10 {
          let lock_result = fs_file.try_lock_exclusive();
          let poll_file_update_ms = 100;
          match lock_result {
            Ok(_) => {
              log_debug!(environment, "Acquired file lock at {}", file_path.display());
              #[allow(clippy::disallowed_methods)]
              let _ignore = std::fs::write(&last_updated_path, "");
              let token = Arc::new(tokio_util::sync::CancellationToken::new());

              // Spawn a blocking task that will continually update a file
              // signalling the lock is alive. This is a fail safe for when
              // a file lock is never released. For example, on some operating
              // systems, if a process does not release the lock (say it's
              // killed), then the OS may release it at an indeterminate time
              //
              // This uses a blocking task because we use a single threaded
              // runtime and this is time sensitive so we don't want it to update
              // at the whims of of whatever is occurring on the runtime thread.
              dprint_core::async_runtime::spawn_blocking({
                let token = token.clone();
                let last_updated_path = last_updated_path.clone();
                move || {
                  let mut i = 0;
                  while !token.is_cancelled() {
                    i += 1;
                    #[allow(clippy::disallowed_methods)]
                    let _ignore = std::fs::write(&last_updated_path, i.to_string());
                    std::thread::sleep(Duration::from_millis(poll_file_update_ms));
                  }
                }
              });

              return Self(Some(LaxSingleProcessFsFlagInner {
                file_path,
                fs_file,
                finished_token: token,
                environment: environment.clone(),
              }));
            }
            Err(_) => {
              // show a message if it's been a while
              if pb_update_guard.is_none() && start_instant.elapsed().as_millis() > 1_000 {
                pb_update_guard = environment
                  .progress_bars()
                  .map(|pb| pb.add_progress(long_wait_message.to_string(), crate::utils::ProgressBarStyle::Action, 1));
              }

              // sleep for a little bit
              tokio::time::sleep(Duration::from_millis(20)).await;

              // Poll the last updated path to check if it's stopped updating,
              // which is an indication that the file lock is claimed, but
              // was never properly released.
              #[allow(clippy::disallowed_methods)]
              match std::fs::metadata(&last_updated_path).and_then(|p| p.modified()) {
                Ok(last_updated_time) => {
                  let current_time = std::time::SystemTime::now();
                  match current_time.duration_since(last_updated_time) {
                    Ok(duration) => {
                      if duration.as_millis() > (poll_file_update_ms * 2) as u128 {
                        // the other process hasn't updated this file in a long time
                        // so maybe it was killed and the operating system hasn't
                        // released the file lock yet
                        return Self(None);
                      } else {
                        error_count = 0; // reset
                      }
                    }
                    Err(_) => {
                      error_count += 1;
                    }
                  }
                }
                Err(_) => {
                  error_count += 1;
                }
              }
            }
          }
        }

        drop(pb_update_guard); // explicit for clarity
        Self(None)
      }
      Err(err) => {
        log_debug!(environment, "Failed to open file lock at {}. {:#}", file_path.display(), err);
        Self(None) // let the process through
      }
    }
  }
}

struct LaxSingleProcessFsFlagInner<TEnvironment: Environment> {
  file_path: PathBuf,
  fs_file: std::fs::File,
  finished_token: Arc<tokio_util::sync::CancellationToken>,
  environment: TEnvironment,
}

impl<TEnvironment: Environment> Drop for LaxSingleProcessFsFlagInner<TEnvironment> {
  fn drop(&mut self) {
    use fs3::FileExt;
    // kill the poll thread
    self.finished_token.cancel();
    // release the file lock
    if let Err(err) = FileExt::unlock(&self.fs_file) {
      log_debug!(self.environment, "Failed releasing lock for {}. {:#}", self.file_path.display(), err);
    }
  }
}

#[cfg(test)]
mod test {
  use std::sync::Arc;

  use dprint_core::async_runtime::future;
  use dprint_core::async_runtime::FutureExt;
  use parking_lot::Mutex;
  use tempfile::TempDir;
  use tokio::sync::Notify;

  use crate::environment::RealEnvironment;

  use super::*;

  #[test]
  fn lax_fs_lock() {
    RealEnvironment::run_test_with_real_env(|env| {
      async move {
        let temp_dir = TempDir::new().unwrap();
        let lock_path = temp_dir.path().join("file.lock");
        let signal1 = Arc::new(Notify::new());
        let signal2 = Arc::new(Notify::new());
        let signal3 = Arc::new(Notify::new());
        let signal4 = Arc::new(Notify::new());
        tokio::spawn({
          let lock_path = lock_path.clone();
          let signal1 = signal1.clone();
          let signal2 = signal2.clone();
          let signal3 = signal3.clone();
          let signal4 = signal4.clone();
          let temp_dir_path = temp_dir.path().to_path_buf();
          let env = env.clone();
          async move {
            let flag = LaxSingleProcessFsFlag::lock(&env, lock_path.to_path_buf(), "waiting").await;
            signal1.notify_one();
            signal2.notified().await;
            tokio::time::sleep(Duration::from_millis(10)).await; // give the other thread time to acquire the lock
            std::fs::write(temp_dir_path.join("file.txt"), "update1").unwrap();
            signal3.notify_one();
            signal4.notified().await;
            drop(flag);
          }
        });
        let signal5 = Arc::new(Notify::new());
        tokio::spawn({
          let temp_dir_path = temp_dir.path().to_path_buf();
          let signal5 = signal5.clone();
          let env = env.clone();
          async move {
            signal1.notified().await;
            signal2.notify_one();
            let flag = LaxSingleProcessFsFlag::lock(&env, lock_path.to_path_buf(), "waiting").await;
            std::fs::write(temp_dir_path.join("file.txt"), "update2").unwrap();
            signal5.notify_one();
            drop(flag);
          }
        });

        signal3.notified().await;
        assert_eq!(std::fs::read_to_string(temp_dir.path().join("file.txt")).unwrap(), "update1");
        signal4.notify_one();
        signal5.notified().await;
        assert_eq!(std::fs::read_to_string(temp_dir.path().join("file.txt")).unwrap(), "update2");
      }
      .boxed_local()
    });
  }

  #[test]
  fn lax_fs_lock_ordered() {
    RealEnvironment::run_test_with_real_env(|env| {
      async move {
        let temp_dir = TempDir::new().unwrap();
        let lock_path = temp_dir.path().join("file.lock");
        let output_path = temp_dir.path().join("output");
        let expected_order = Arc::new(Mutex::new(Vec::new()));
        let count = 10;
        let mut tasks = Vec::with_capacity(count);

        std::fs::write(&output_path, "").unwrap();

        for i in 0..count {
          let lock_path = lock_path.clone();
          let output_path = output_path.clone();
          let expected_order = expected_order.clone();
          let env = env.clone();
          tasks.push(tokio::spawn(async move {
            let flag = LaxSingleProcessFsFlag::lock(&env, lock_path.to_path_buf(), "waiting").await;
            expected_order.lock().push(i.to_string());
            // be extremely racy
            let mut output = std::fs::read_to_string(&output_path).unwrap();
            if !output.is_empty() {
              output.push('\n');
            }
            output.push_str(&i.to_string());
            std::fs::write(&output_path, output).unwrap();
            drop(flag);
          }));
        }

        future::join_all(tasks).await;
        let expected_output = expected_order.lock().join("\n");
        assert_eq!(std::fs::read_to_string(output_path).unwrap(), expected_output);
      }
      .boxed_local()
    })
  }
}
