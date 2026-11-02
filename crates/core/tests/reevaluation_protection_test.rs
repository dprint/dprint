use std::cell::Cell;

use dprint_core::formatting::IntoPrintItems;
use dprint_core::formatting::PrintItems;
use dprint_core::formatting::PrintOptions;
use dprint_core::formatting::conditions::if_true_or;

#[test]
fn should_stabilize_after_reevaluation_flipping() {
  let result = dprint_core::formatting::format(
    |arena| {
      let mut items = PrintItems::new(arena);
      let cell: &Cell<bool> = arena.bump().alloc(Cell::new(false));
      // cause an infinite loop
      let mut condition = if_true_or(
        "flipping",
        arena.alloc_resolver(move |_| {
          // flip forever
          cell.set(!cell.get());
          Some(cell.get())
        }),
        "1".into_print_items(arena),
        "2".into_print_items(arena),
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
