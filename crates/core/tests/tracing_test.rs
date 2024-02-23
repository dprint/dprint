#[test]
#[cfg(feature = "tracing")]
fn test_tracing() {
  use dprint_core::formatting::*;

  let trace_result = trace_printing(
    || {
      let mut print_items = PrintItems::new();
      print_items.push_info(LineNumber::new("line_number"));
      print_items.push_signal(Signal::NewLine);
      print_items.push_str_runtime_width_computed("string");
      print_items.push_condition(conditions::if_true_or(
        "condition_name",
        std::rc::Rc::new(|_| Some(true)),
        "true_path".into(),
        "false_path".into(),
      ));
      print_items.push_optional_path({
        let mut other_print_items = PrintItems::new();
        other_print_items.push_signal(Signal::SingleIndent);
        other_print_items.into_rc_path()
      });
      print_items
    },
    PrintOptions {
      indent_width: 4,
      use_tabs: false,
      max_width: 80,
      new_line_text: "\n",
    },
  );

  // very basic test just to ensure it's working
  assert_eq!(trace_result.print_nodes.len(), 8);
  assert_eq!(trace_result.traces.len(), 7);
  assert_eq!(trace_result.writer_nodes.len(), 4);
}
