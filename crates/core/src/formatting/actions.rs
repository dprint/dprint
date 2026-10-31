use super::Arena;
use super::print_items::*;

pub fn if_column_number_changes<'a>(arena: &'a Arena, inner_action: impl Fn(&mut ConditionResolverContext) + Copy + 'a) -> PrintItems<'a> {
  let mut items = PrintItems::new(arena);
  let column_number = ColumnNumber::new("columnNumber");
  items.extend(action(arena, "actionIfColChanges", move |context| {
    if let Some(column_number) = context.resolved_column_number(column_number)
      && column_number != context.writer_info.column_number
    {
      inner_action(context);
    }
  }));
  items.push_info(column_number);
  items
}

/// Runs the action when the printer reaches this point.
///
/// The action needs to be `Copy` (see [`Arena::alloc_resolver`]).
pub fn action<'a>(arena: &'a Arena, name: &'static str, action: impl Fn(&mut ConditionResolverContext) + Copy + 'a) -> PrintItems<'a> {
  Condition::new(
    name,
    ConditionProperties {
      condition: arena.alloc_resolver(move |context| {
        action(context);
        Some(true)
      }),
      true_path: None,
      false_path: None,
    },
  )
  .into_print_items(arena)
}
