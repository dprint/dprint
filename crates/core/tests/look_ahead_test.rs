use dprint_core::formatting::*;

#[test]
fn should_not_go_back_to_a_condition_taking_the_same_path() {
  // the condition is printed while the line number isn't resolved, which is the same as false
  fn generate<'a>(arena: &'a Arena) -> PrintItems<'a> {
    let end_ln = LineNumber::new("end");
    let mut items = PrintItems::new(arena);
    items.push_condition(conditions::if_true(
      "newLineIfEndOnLaterLine",
      arena.alloc_resolver(move |context| Some(context.resolved_line_number(end_ln)? > 0)),
      Signal::NewLine.into_print_items(arena),
    ));
    items.push_str_runtime_width_computed("a");
    items.push_info(end_ln);
    items
  }

  assert_eq!(format(generate, print_options()), "a");
  assert_jump_count(generate, 0);
}

#[test]
fn should_go_back_to_a_condition_still_looking_ahead_when_it_changes() {
  // the condition is still unknown once the first line number is resolved, then the second one makes it true
  fn generate<'a>(arena: &'a Arena) -> PrintItems<'a> {
    let first_ln = LineNumber::new("first");
    let second_ln = LineNumber::new("second");
    let mut items = PrintItems::new(arena);
    items.push_condition(conditions::if_true(
      "markIfSecondOnLaterLine",
      arena.alloc_resolver(move |context| {
        let first = context.resolved_line_number(first_ln)?;
        let second = context.resolved_line_number(second_ln)?;
        Some(second > first)
      }),
      "x".into_print_items(arena),
    ));
    items.push_info(first_ln);
    items.push_str_runtime_width_computed("a");
    items.push_signal(Signal::NewLine);
    items.push_str_runtime_width_computed("b");
    items.push_info(second_ln);
    items
  }

  assert_eq!(format(generate, print_options()), "xa\nb");
  assert_jump_count(generate, 1);
}

#[test]
fn should_go_back_when_another_condition_looked_ahead_at_the_same_info() {
  // only the first condition goes back once the line number is resolved, which prints the second
  // one again, and the second one changes even though the first one doesn't
  fn generate<'a>(arena: &'a Arena) -> PrintItems<'a> {
    let end_ln = LineNumber::new("end");
    let mut items = PrintItems::new(arena);
    items.push_condition(conditions::if_true(
      "markIfEndOnLaterLine",
      arena.alloc_resolver(move |context| Some(context.resolved_line_number(end_ln)? > 0)),
      "1".into_print_items(arena),
    ));
    items.push_condition(conditions::if_true(
      "markIfEndOnFirstLine",
      arena.alloc_resolver(move |context| Some(context.resolved_line_number(end_ln)? == 0)),
      "2".into_print_items(arena),
    ));
    items.push_str_runtime_width_computed("a");
    items.push_info(end_ln);
    items
  }

  assert_eq!(format(generate, print_options()), "2a");
  assert_jump_count(generate, 1);
}

#[cfg(feature = "tracing")]
fn assert_jump_count(generate: impl for<'a> FnOnce(&'a Arena) -> PrintItems<'a>, expected: usize) {
  let arena = Arena::new();
  let result = trace_printing(generate(&arena), print_options());
  let jump_count = result.traces.iter().filter(|trace| trace.jump.is_some()).count();
  assert_eq!(jump_count, expected);
}

#[cfg(not(feature = "tracing"))]
fn assert_jump_count(_generate: impl for<'a> FnOnce(&'a Arena) -> PrintItems<'a>, _expected: usize) {}

fn print_options() -> PrintOptions {
  PrintOptions {
    indent_width: 2,
    max_width: 40,
    use_tabs: false,
    new_line_text: "\n",
  }
}
