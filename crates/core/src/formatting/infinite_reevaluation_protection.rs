use super::collections::VecU32MapWithDefault;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

static LOGGED: AtomicBool = AtomicBool::new(false);

/// This provides some protection if a condition re-evaluation keeps
/// flipping back and forth over and over.
pub struct InfiniteReevaluationProtector {
  reevaluation_count: VecU32MapWithDefault<u16>,
}

impl InfiniteReevaluationProtector {
  pub fn with_capacity(capacity: u32) -> Self {
    Self {
      reevaluation_count: VecU32MapWithDefault::with_capacity(capacity),
    }
  }

  pub fn should_reevaluate(&mut self, reevaluation_id: u32, current_value: Option<bool>, last_value: bool) -> bool {
    const MAX_COUNT: u16 = 1_000;
    let current_value = current_value.unwrap_or(false);
    if current_value == last_value {
      self.reevaluation_count.set(reevaluation_id, 0); // reset
      true
    } else {
      // re-evaluation flipped
      let next_count = self.reevaluation_count.get(reevaluation_id).unwrap() + 1;
      if next_count == MAX_COUNT + 1 {
        return false;
      }

      self.reevaluation_count.set(reevaluation_id, next_count);
      if next_count == MAX_COUNT {
        // only ever log this once per execution
        if !LOGGED.swap(true, Ordering::SeqCst) {
          eprintln!(
            "[dprint-core] A file exceeded the re-evaluation count and formatting stabilized at a random condition value. Please report this as a bug."
          );
        }
        false
      } else {
        true
      }
    }
  }
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn should_keep_track_flipping_reevaluation() {
    let mut protector = InfiniteReevaluationProtector::with_capacity(1);
    for _ in 0..999 {
      assert!(protector.should_reevaluate(0, Some(true), false));
    }

    assert!(!protector.should_reevaluate(0, Some(true), false));
    assert!(!protector.should_reevaluate(0, Some(true), false));
    assert!(protector.should_reevaluate(0, Some(true), true));
  }

  #[test]
  fn should_reset_after_not_flipping() {
    let mut protector = InfiniteReevaluationProtector::with_capacity(10);
    let mut value = false;
    for _ in 0..998 {
      value = !value;
      assert!(protector.should_reevaluate(0, Some(true), false));
    }
    assert!(protector.should_reevaluate(0, None, false));

    for _ in 0..999 {
      assert!(protector.should_reevaluate(0, Some(false), true));
    }

    assert!(!protector.should_reevaluate(0, Some(true), false));
  }
}
