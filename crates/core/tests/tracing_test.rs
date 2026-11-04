#[test]
#[cfg(feature = "tracing")]
fn test_tracing() {
  use dprint_core::formatting::*;

  let arena = Arena::new();
  let trace_result = trace_printing(
    {
      let arena = &arena;
      let mut print_items = PrintItems::new(arena);
      print_items.push_info(LineNumber::new("line_number"));
      print_items.push_signal(Signal::NewLine);
      print_items.push_str_runtime_width_computed("string");
      print_items.push_condition(conditions::if_true_or(
        "condition_name",
        arena.alloc_resolver(|_| Some(true)),
        "true_path".into_print_items(arena),
        "false_path".into_print_items(arena),
      ));
      print_items.push_optional_path({
        let mut other_print_items = PrintItems::new(arena);
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
  assert!(trace_result.traces.iter().all(|trace| trace.jump.is_none()));
}

#[test]
// the names of infos and conditions are only kept in debug builds
#[cfg(all(feature = "tracing", debug_assertions))]
fn test_tracing_jumps() {
  use dprint_core::formatting::*;

  let arena = Arena::new();
  let trace_result = trace_printing(
    {
      let arena = &arena;
      let mut print_items = PrintItems::new(arena);
      let start_ln = LineNumber::new("start");
      let end_ln = LineNumber::new("end");
      let mut later_condition = conditions::if_true("laterCondition", arena.alloc_resolver(|_| Some(true)), PrintItems::new(arena));
      let later_condition_reference = later_condition.create_reference();
      print_items.push_info(start_ln);
      // prints text until the later condition is resolved
      print_items.push_condition(conditions::if_true_or(
        "usesLaterCondition",
        arena.alloc_resolver(move |context| context.resolved_condition(&later_condition_reference)),
        PrintItems::new(arena),
        "xxxxx".into_print_items(arena),
      ));
      let mut multi_line_condition = conditions::if_true(
        "isMultiLine",
        arena.alloc_resolver(move |context| {
          let start = context.resolved_line_number(start_ln)?;
          let end = context.resolved_line_number(end_ln)?;
          Some(end > start)
        }),
        "a".into_print_items(arena),
      );
      let multi_line_reevaluation = multi_line_condition.create_reevaluation();
      print_items.push_condition(multi_line_condition);
      print_items.push_signal(Signal::PossibleNewLine);
      print_items.push_str_runtime_width_computed("67890");
      print_items.push_info(end_ln);
      print_items.push_condition(later_condition);
      print_items.push_reevaluation(multi_line_reevaluation);
      print_items
    },
    PrintOptions {
      indent_width: 4,
      use_tabs: false,
      max_width: 8,
      new_line_text: "\n",
    },
  );

  let jumps = trace_result
    .traces
    .iter()
    .filter_map(|trace| trace.jump.as_ref())
    .map(describe_jump)
    .collect::<Vec<_>>();
  assert_eq!(
    jumps,
    vec![
      // "xxxxx67890" is too long
      "newLine",
      // isMultiLine used the end line before it was resolved
      "info end 1",
      // "xxxxxa67890" is too long
      "newLine",
      // usesLaterCondition used laterCondition before it was resolved
      "condition laterCondition Some(true)",
      // "a67890" fits, so the end line moved up and isMultiLine changed
      "conditionReevaluation isMultiLine Some(false)",
    ]
  );

  fn describe_jump(jump: &TraceJump) -> String {
    match jump {
      TraceJump::NewLine => "newLine".to_string(),
      TraceJump::Info(jump) => {
        let name = match &jump.info {
          TraceInfo::LineNumber(info)
          | TraceInfo::ColumnNumber(info)
          | TraceInfo::IsStartOfLine(info)
          | TraceInfo::IndentLevel(info)
          | TraceInfo::LineStartColumnNumber(info)
          | TraceInfo::LineStartIndentLevel(info) => &info.name,
        };
        let value = match jump.value {
          TraceInfoValue::Number(value) => value.to_string(),
          TraceInfoValue::Bool(value) => value.to_string(),
        };
        format!("info {} {}", name, value)
      }
      TraceJump::Condition(jump) => format!("condition {} {:?}", jump.name, jump.value),
      TraceJump::ConditionReevaluation(jump) => format!("conditionReevaluation {} {:?}", jump.name, jump.value),
    }
  }
}
