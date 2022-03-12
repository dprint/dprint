use std::collections::HashSet;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::environment::Environment;
use crate::utils::ThreadExitSignal;

use super::WorkerRegistry;

/// Thread that periodically checks workers to ensure a file
/// path isn't taking too long to format.
pub struct LongFormatCheckerThread<TEnvironment: Environment> {
  environment: TEnvironment,
  worker_registry: Arc<WorkerRegistry<TEnvironment>>,
  thread_exit_signal: Arc<ThreadExitSignal>,
}

impl<TEnvironment: Environment> LongFormatCheckerThread<TEnvironment> {
  pub fn new(environment: &TEnvironment, worker_registry: Arc<WorkerRegistry<TEnvironment>>) -> Self {
    LongFormatCheckerThread {
      environment: environment.clone(),
      worker_registry,
      thread_exit_signal: Arc::new(ThreadExitSignal::new()),
    }
  }

  /// Spawns a thread to watch the workers.
  pub fn spawn(&self) {
    let exit_signal = self.thread_exit_signal.clone();
    let worker_registry = self.worker_registry.clone();
    let environment = self.environment.clone();
    let mut logged_file_paths = HashSet::new();
    tokio::task::spawn(move || {
      // initially sleep 10 seconds
      if !exit_signal.sleep_with_cancellation(Duration::from_secs(10)) {
        return;
      }

      // now check each worker every 2.5 seconds for files that have been formatting for more than 10 seconds
      loop {
        if !exit_signal.sleep_with_cancellation(Duration::from_millis(2_500)) {
          return;
        }

        // get each worker's current file path and log file paths that are taking a long time to format
        for worker in worker_registry.workers.iter() {
          if let Some(file_path_info) = worker.get_current_formatting_file_path_info() {
            if file_path_info.start_time.elapsed() > Duration::from_secs(10) {
              // log if it hasn't been logged before
              if !logged_file_paths.contains(&file_path_info.file_path) {
                environment.log_stderr(&format!("WARNING: Formatting is slow for {}", file_path_info.file_path.display()));
                logged_file_paths.insert(file_path_info.file_path.clone());
              }
            }
          }
        }
      }
    });
  }

  pub fn signal_exit(&self) {
    self.thread_exit_signal.signal_exit();
  }
}
