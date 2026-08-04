use std::cell::Cell;
use std::cell::RefCell;
use std::rc::Rc;

use dprint_core::formatting::PrintItems;
use dprint_core::formatting::PrintOptions;
use dprint_core::formatting::actions::action;
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

#[test]
fn should_not_reprint_when_future_condition_stays_on_false_path() {
  let false_path_count = Rc::new(Cell::new(0));
  let result = dprint_core::formatting::format(
    || {
      let mut items = PrintItems::new();
      let mut future_condition = if_true_or("futureCondition", Rc::new(|_| Some(false)), "futureTrue".into(), "futureFalse".into());
      let future_condition_ref = future_condition.create_reference();
      let mut false_path = PrintItems::new();
      let false_path_count = false_path_count.clone();
      false_path.extend(action("countFalsePath", move |_| {
        false_path_count.set(false_path_count.get() + 1);
      }));
      false_path.push_str_runtime_width_computed("false");
      items.push_condition(if_true_or(
        "dependsOnFutureCondition",
        future_condition_ref.create_resolver(),
        "true".into(),
        false_path,
      ));
      items.push_condition(future_condition);
      items
    },
    PrintOptions {
      indent_width: 2,
      max_width: 40,
      use_tabs: false,
      new_line_text: "\n",
    },
  );

  assert_eq!(result, "falsefutureFalse");
  assert_eq!(false_path_count.get(), 1);
}
