use super::collections::VecU32MapWithDefault;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

static LOGGED: AtomicBool = AtomicBool::new(false);

/// This provides some protection if a condition re-evaluation keeps
/// happening indefinitely for any reason.
pub struct InfiniteReevaluationProtector {
  total_count: VecU32MapWithDefault<u16>,
}

impl InfiniteReevaluationProtector {
  pub fn with_capacity(capacity: u32) -> Self {
    Self {
      total_count: VecU32MapWithDefault::with_capacity(capacity),
    }
  }

  /// Checks if a condition should be re-evaluated by counting total re-evaluations.
  /// Protects against infinite loops from any cause (flipping values, never-resolving
  /// conditions, circular dependencies, etc.).
  ///
  /// Returns true if re-evaluation should continue, false if limit is exceeded.
  pub fn should_reevaluate(&mut self, reevaluation_id: u32, current_value: Option<bool>, last_value: bool) -> bool {
    const MAX_REEVALUATIONS: u16 = 500;

    let current_value = current_value.unwrap_or(false);
    if current_value == last_value {
      // Value stabilized, reset counter
      self.total_count.set(reevaluation_id, 0);
      return true;
    }

    // Increment total re-evaluation count
    let count = self.total_count.get(reevaluation_id).unwrap() + 1;
    self.total_count.set(reevaluation_id, count);

    if count >= MAX_REEVALUATIONS {
      // only ever log this once per execution
      if !LOGGED.swap(true, Ordering::SeqCst) {
        // todo: use awasm logging here instead
        #[allow(clippy::print_stderr)]
        {
          eprintln!(
            "[dprint-core] A condition was re-evaluated {} times without stabilizing. This indicates an infinite re-evaluation loop. Please report this as a bug.",
            MAX_REEVALUATIONS
          );
        }
      }
      false
    } else {
      true
    }
  }
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn should_keep_track_flipping_reevaluation() {
    let mut protector = InfiniteReevaluationProtector::with_capacity(1);
    for _ in 0..499 {
      assert!(protector.should_reevaluate(0, Some(true), false));
    }

    assert!(!protector.should_reevaluate(0, Some(true), false));
    assert!(!protector.should_reevaluate(0, Some(true), false));
    assert!(protector.should_reevaluate(0, Some(true), true));
  }

  #[test]
  fn should_reset_after_not_flipping() {
    let mut protector = InfiniteReevaluationProtector::with_capacity(10);
    for _ in 0..498 {
      assert!(protector.should_reevaluate(0, Some(true), false));
    }
    // When value stabilizes, counter resets
    assert!(protector.should_reevaluate(0, Some(false), false));

    // Can continue for another 499 re-evaluations
    for _ in 0..499 {
      assert!(protector.should_reevaluate(0, Some(false), true));
    }

    assert!(!protector.should_reevaluate(0, Some(true), false));
  }
}
