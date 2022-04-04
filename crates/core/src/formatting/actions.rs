use std::rc::Rc;

use super::print_items::*;

pub fn if_column_number_changes(action: impl Fn(&mut ConditionResolverContext) + 'static) -> PrintItems {
  let mut items = PrintItems::new();
  let column_number = ColumnNumber::new("columnNumber");
  items.push_condition(Condition::new(
    "actionIfColChanges",
    ConditionProperties {
      condition: Rc::new(move |context| {
        let column_number = context.get_resolved_column_number(column_number)?;
        if column_number != context.writer_info.column_number {
          action(context);
        }
        Some(true)
      }),
      true_path: None,
      false_path: None,
    },
  ));
  items.push_column_number(column_number);
  items
}
