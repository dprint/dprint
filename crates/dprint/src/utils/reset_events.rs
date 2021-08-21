use parking_lot::{Condvar, Mutex};
use std::time::Duration;

/// A thread synchronization event that, when signaled, must be reset manually.
///
/// This is a very loose Rust implementation of C#'s ManualResetEvent.
pub struct ManualResetEvent {
  is_set: (Mutex<bool>, Condvar),
}

impl ManualResetEvent {
  pub fn new(initial_state: bool) -> ManualResetEvent {
    ManualResetEvent {
      is_set: (Mutex::new(initial_state), Condvar::new()),
    }
  }

  /// Waits for the event to be set with a timeout.
  ///
  /// Returns true if the event was set and false if the timeout was exceeded.
  pub fn wait_with_timeout(&self, timeout: Duration) -> bool {
    let &(ref lock, ref cvar) = &self.is_set;
    let mut is_set = lock.lock();
    if !*is_set {
      cvar.wait_for(&mut is_set, timeout);
    }

    *is_set
  }

  /// Sets the event.
  pub fn set(&self) {
    let &(ref lock, ref cvar) = &self.is_set;
    let mut is_set = lock.lock();
    *is_set = true;
    cvar.notify_all();
  }

  /// Resets the event.
  #[allow(dead_code)]
  pub fn reset(&self) {
    let &(ref lock, _) = &self.is_set;
    let mut is_set = lock.lock();
    *is_set = false;
  }
}
