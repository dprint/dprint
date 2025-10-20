use std::cell::RefCell;
use std::rc::Rc;

use dprint_core::formatting::conditions::if_true_or;
use dprint_core::formatting::PrintItems;
use dprint_core::formatting::PrintOptions;

#[test]
fn should_stabilize_after_reevaluation_flipping() {
  let result = dprint_core::formatting::format(
    || {
      let mut items = PrintItems::new();
      let cell = RefCell::new(false);
      // cause an infinite loop
      let mut condition = if_true_or(
        "flipping",
        Rc::new(move |_| {
          // flip forever
          let mut value = cell.borrow_mut();
          *value = !*value;
          Some(*value)
        }),
        "1".into(),
        "2".into(),
      );
      let reevaluation = condition.create_reevaluation();
      items.push_condition(condition);
      items.push_reevaluation(reevaluation);
      items
    },
    PrintOptions {
      indent_width: 2,
      max_width: 40,
      use_tabs: false,
      new_line_text: "\n",
    },
  );
  assert_eq!(result, "1");
}

#[test]
fn should_stabilize_when_condition_never_resolves() {
  let result = dprint_core::formatting::format(
    || {
      let mut items = PrintItems::new();
      // Create a condition that always returns None (never resolves)
      let mut condition = if_true_or(
        "never_resolves",
        Rc::new(move |_| {
          // Never resolve - always return None
          None
        }),
        "1".into(),
        "2".into(),
      );
      let reevaluation = condition.create_reevaluation();
      items.push_condition(condition);
      items.push_reevaluation(reevaluation);
      items
    },
    PrintOptions {
      indent_width: 2,
      max_width: 40,
      use_tabs: false,
      new_line_text: "\n",
    },
  );
  // Should stabilize at some value instead of looping forever
  assert!(result == "1" || result == "2");
}
