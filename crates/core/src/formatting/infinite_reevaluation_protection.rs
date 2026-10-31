use super::collections::VecU32MapWithDefault;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

static LOGGED: AtomicBool = AtomicBool::new(false);

/// The number of flips after which a condition is stopped from being re-evaluated.
const MAX_COUNT: u16 = 1_000;
/// The number of flips after which a condition is reported as flipping when a
/// condition is stopped, which is more than a condition that stabilizes needs.
const FLIPPING_COUNT: u16 = 10;

/// This provides some protection if a condition re-evaluation keeps
/// flipping back and forth over and over.
pub struct InfiniteReevaluationProtector {
  reevaluation_count: VecU32MapWithDefault<u16>,
  /// Names of the conditions that flipped at least `FLIPPING_COUNT` times, which
  /// are the conditions in a cycle of conditions that depend on each other.
  flipping_condition_names: Vec<&'static str>,
}

impl InfiniteReevaluationProtector {
  pub fn with_capacity(capacity: u32) -> Self {
    Self {
      reevaluation_count: VecU32MapWithDefault::with_capacity(capacity),
      flipping_condition_names: Vec::new(),
    }
  }

  pub fn should_reevaluate(&mut self, reevaluation_id: u32, condition_name: &'static str, current_value: Option<bool>, last_value: bool) -> bool {
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
      if next_count == FLIPPING_COUNT && !self.flipping_condition_names.contains(&condition_name) {
        self.flipping_condition_names.push(condition_name);
      }
      if next_count == MAX_COUNT {
        // only ever log this once per execution
        if !LOGGED.swap(true, Ordering::SeqCst) {
          // todo: use awasm logging here instead
          #[allow(clippy::print_stderr)]
          {
            eprintln!("{}", self.exceeded_message(condition_name));
          }
        }
        false
      } else {
        true
      }
    }
  }

  fn exceeded_message(&self, _stopped_condition_name: &'static str) -> String {
    #[allow(unused_mut)]
    let mut message =
      String::from("[dprint-core] A file exceeded the re-evaluation count and formatting stabilized at a random condition value. Please report this as a bug.");
    // the names are only kept in debug builds
    #[cfg(debug_assertions)]
    {
      message.push_str(&format!(
        "\n  Stopped condition: {}\n  Conditions that kept flipping: {}",
        _stopped_condition_name,
        self.flipping_condition_names.join(", ")
      ));
    }
    message
  }
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn should_keep_track_flipping_reevaluation() {
    let mut protector = InfiniteReevaluationProtector::with_capacity(1);
    for _ in 0..999 {
      assert!(protector.should_reevaluate(0, "a", Some(true), false));
    }

    assert!(!protector.should_reevaluate(0, "a", Some(true), false));
    assert!(!protector.should_reevaluate(0, "a", Some(true), false));
    assert!(protector.should_reevaluate(0, "a", Some(true), true));
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
      assert!(protector.should_reevaluate(0, "a", Some(true), false));
      // ...followed by a non-flip, which must not reset the accumulated count
      assert!(protector.should_reevaluate(0, "a", Some(true), true));
    }

    // the 1000th flip stabilizes the value and stops further re-evaluation
    assert!(!protector.should_reevaluate(0, "a", Some(true), false));
    assert!(!protector.should_reevaluate(0, "a", Some(true), false));
  }

  #[test]
  fn should_name_the_flipping_conditions() {
    let mut protector = InfiniteReevaluationProtector::with_capacity(3);
    // a condition that stabilizes after a few flips isn't named
    for _ in 0..FLIPPING_COUNT - 1 {
      protector.should_reevaluate(2, "stabilizes", Some(true), false);
    }
    // two conditions that depend on each other keep flipping
    for _ in 0..MAX_COUNT - 1 {
      protector.should_reevaluate(0, "first", Some(true), false);
      protector.should_reevaluate(1, "second", Some(true), false);
    }

    let message = protector.exceeded_message("first");
    if cfg!(debug_assertions) {
      assert!(
        message.ends_with("\n  Stopped condition: first\n  Conditions that kept flipping: first, second"),
        "{}",
        message
      );
    } else {
      assert!(!message.contains("first"), "{}", message);
    }
  }
}
