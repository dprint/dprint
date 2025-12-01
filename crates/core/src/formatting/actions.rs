use std::rc::Rc;

use super::print_items::*;

pub fn if_column_number_changes(inner_action: impl Fn(&mut ConditionResolverContext) + 'static) -> PrintItems {
  let mut items = PrintItems::new();
  let column_number = ColumnNumber::new("columnNumber");
  items.extend(action("actionIfColChanges", move |context| {
    if let Some(column_number) = context.resolved_column_number(column_number)
      && column_number != context.writer_info.column_number
    {
      inner_action(context);
    }
  }));
  items.push_info(column_number);
  items
}

pub fn action(name: &'static str, action: impl Fn(&mut ConditionResolverContext) + 'static) -> PrintItems {
  Condition::new(
    name,
    ConditionProperties {
      condition: Rc::new(move |context| {
        action(context);
        Some(true)
      }),
      true_path: None,
      false_path: None,
    },
  )
  .into()
}
