use super::ManualResetEvent;
use std::time::Duration;

/// Provides a way to signal to a thread that it should exit
/// along with providing a way for the thread to sleep and be
/// signaled for exit while sleeping.
pub struct ThreadExitSignal {
  is_complete: ManualResetEvent,
}

impl ThreadExitSignal {
  pub fn new() -> Self {
    ThreadExitSignal {
      is_complete: ManualResetEvent::new(false),
    }
  }

  /// Sleeps the thread, exiting when the signal is given or the duration has passed.
  ///
  /// Returns `true` when it has slept for the duration of the timeout.
  pub fn sleep_with_cancellation(&self, duration: Duration) -> bool {
    !self.is_complete.wait_with_timeout(duration)
  }

  /// Signal that the thread should exit.
  pub fn signal_exit(&self) {
    self.is_complete.set();
  }
}

#[cfg(test)]
mod test {
  use super::super::ManualResetEvent;
  use super::*;
  use std::sync::Arc;
  use std::time::Duration;

  #[test]
  fn should_signal_a_thread_to_stop_sleeping() {
    let exit_signal = Arc::new(ThreadExitSignal::new());
    let has_thread_exited = Arc::new(ManualResetEvent::new(false));

    // should sleep and not be cancelled
    assert_eq!(exit_signal.sleep_with_cancellation(Duration::from_millis(1)), true);

    std::thread::spawn({
      let exit_signal = exit_signal.clone();
      let has_thread_exited = has_thread_exited.clone();
      move || {
        assert_eq!(exit_signal.sleep_with_cancellation(Duration::from_secs(5)), false);
        has_thread_exited.set();
      }
    });

    // give the thread some time to start
    std::thread::sleep(Duration::from_millis(100));

    // now tell it to exit
    exit_signal.signal_exit();

    // ensure the thread exits before the timeout
    assert_eq!(has_thread_exited.wait_with_timeout(Duration::from_secs(5)), true);
  }
}
