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
      // The condition resolved to the same value it had before, so on its own
      // it isn't flipping right now.
      //
      // Note: this used to reset the flip counter to zero. That allowed two (or
      // more) conditions whose values depend on each other to ping-pong
      // forever: each one alternates between flipping and not flipping, so its
      // per-condition counter was reset on every other re-evaluation and never
      // reached `MAX_COUNT`. The printer would then keep re-evaluating and
      // backtracking indefinitely, allocating save points until it ran out of
      // memory. Keeping the accumulated count guarantees a condition that keeps
      // flipping is eventually stopped regardless of the cycle it's part of.
      // A condition that is going to stabilize does so after only a handful of
      // flips, so this never affects formatting of well-behaved files.
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
          // todo: use awasm logging here instead
          #[allow(clippy::print_stderr)]
          {
            eprintln!(
              "[dprint-core] A file exceeded the re-evaluation count and formatting stabilized at a random condition value. Please report this as a bug."
            );
          }
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
  fn should_count_flips_cumulatively_across_non_flips() {
    // Two conditions that depend on each other can ping-pong: a given
    // condition alternates between flipping and resolving to the same value.
    // The flip count must accumulate across the interspersed non-flips,
    // otherwise such a cycle would never be stopped (see denoland/deno#26713).
    let mut protector = InfiniteReevaluationProtector::with_capacity(1);
    for _ in 0..999 {
      // a flip...
      assert!(protector.should_reevaluate(0, Some(true), false));
      // ...followed by a non-flip, which must not reset the accumulated count
      assert!(protector.should_reevaluate(0, Some(true), true));
    }

    // the 1000th flip stabilizes the value and stops further re-evaluation
    assert!(!protector.should_reevaluate(0, Some(true), false));
    assert!(!protector.should_reevaluate(0, Some(true), false));
  }
}
