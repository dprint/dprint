use std::cell::RefCell;
use std::rc::Rc;

use dprint_core::formatting::PrintItems;
use dprint_core::formatting::PrintOptions;
use dprint_core::formatting::conditions::if_true_or;

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
