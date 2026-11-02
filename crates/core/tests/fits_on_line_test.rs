use dprint_core::formatting::*;

/// Generates items after a condition that measures whether they fit on the line
/// up to `end` and writes "|" when they do or a newline when they don't.
fn format_measured(max_width: u32, stop_at_first_break: bool, generate: impl for<'a> Fn(&'a Arena, &mut PrintItems<'a>, LineNumber)) -> String {
  format(
    |arena| {
      let mut items = PrintItems::new(arena);
      let end_ln = LineNumber::new("end");
      let mut fits_condition = Condition::new(
        "fits",
        ConditionProperties {
          condition: arena.alloc_resolver(move |context| {
            if stop_at_first_break {
              context.fits_on_line_until_first_break(end_ln)
            } else {
              context.fits_on_line_until(end_ln)
            }
          }),
          true_path: None,
          false_path: None,
        },
      );
      let fits_ref = fits_condition.create_reference();
      items.push_condition(fits_condition);
      // while measuring, it's not known yet, so it's on one line
      items.push_condition(conditions::if_true_or(
        "newLineIfDoesNotFit",
        arena.alloc_resolver(move |context| context.resolved_condition(&fits_ref).map(|fits| !fits)),
        Signal::NewLine.into_print_items(arena),
        "|".into_print_items(arena),
      ));
      generate(arena, &mut items, end_ln);
      items
    },
    PrintOptions {
      indent_width: 2,
      max_width,
      use_tabs: false,
      new_line_text: "\n",
    },
  )
}

#[test]
fn fits_when_items_up_to_end_fit() {
  let text = format_measured(10, false, |_, items, end_ln| {
    items.push_str_runtime_width_computed("abcd");
    items.push_info(end_ln);
  });
  assert_eq!(text, "|abcd");
}

#[test]
fn does_not_fit_when_items_up_to_end_are_too_wide() {
  let text = format_measured(10, false, |_, items, end_ln| {
    items.push_str_runtime_width_computed("abcdefghijkl");
    items.push_info(end_ln);
  });
  assert_eq!(text, "\nabcdefghijkl");
}

#[test]
fn measures_items_on_one_line() {
  // the newline would make it fit, but the items up to the end are measured on one line
  let text = format_measured(10, false, |_, items, end_ln| {
    items.push_str_runtime_width_computed("abcdef");
    items.push_signal(Signal::SpaceOrNewLine);
    items.push_str_runtime_width_computed("ghijkl");
    items.push_info(end_ln);
  });
  assert_eq!(text, "\nabcdef\nghijkl");
}

#[test]
fn does_not_fit_with_newline_before_end() {
  let text = format_measured(10, false, |_, items, end_ln| {
    items.push_str_runtime_width_computed("ab");
    items.push_signal(Signal::NewLine);
    items.push_str_runtime_width_computed("cd");
    items.push_info(end_ln);
  });
  assert_eq!(text, "\nab\ncd");
}

#[test]
fn measures_rest_of_line_after_end() {
  let text = format_measured(10, false, |_, items, end_ln| {
    items.push_str_runtime_width_computed("abcd");
    items.push_info(end_ln);
    items.push_str_runtime_width_computed("efghijkl");
  });
  assert_eq!(text, "\nabcdefghijkl");
}

#[test]
fn rest_of_line_ends_at_possible_newline() {
  let text = format_measured(10, false, |_, items, end_ln| {
    items.push_str_runtime_width_computed("abcd");
    items.push_info(end_ln);
    items.push_signal(Signal::PossibleNewLine);
    items.push_str_runtime_width_computed("efghijkl");
  });
  assert_eq!(text, "|abcd\nefghijkl");
}

#[test]
fn rest_of_line_continues_past_possible_newline_in_deeper_new_line_group() {
  // the printer would break at an earlier possible newline before one in a deeper new line group
  fn generate<'a>(arena: &'a Arena, items: &mut PrintItems<'a>, end_ln: LineNumber) {
    items.push_str_runtime_width_computed("abcd");
    items.push_info(end_ln);
    items.extend(ir_helpers::new_line_group({
      let mut items = PrintItems::new(arena);
      items.push_signal(Signal::PossibleNewLine);
      items.push_str_runtime_width_computed("efghijkl");
      items
    }));
  }
  // it doesn't fit, then the line breaks in the new line group because it's the only place it can
  assert_eq!(format_measured(10, false, generate), "\nabcd\nefghijkl");
  // unless measuring up to the first possible newline
  assert_eq!(format_measured(10, true, generate), "|abcd\nefghijkl");
}

#[test]
fn text_without_newlines_does_not_count() {
  // like a trailing comment, which can't make the printer break the line either
  let text = format_measured(10, false, |arena, items, end_ln| {
    items.push_str_runtime_width_computed("abcd");
    items.push_info(end_ln);
    items.extend(ir_helpers::with_no_new_lines({
      let mut items = PrintItems::new(arena);
      items.push_str_runtime_width_computed(" // comment");
      items.push_signal(Signal::ExpectNewLine);
      items
    }));
  });
  assert_eq!(text, "|abcd // comment");
}

#[test]
fn restores_what_was_resolved_while_measuring() {
  // a condition after the measured items looks back at a line number resolved while measuring
  let text = format(
    |arena| {
      let mut items = PrintItems::new(arena);
      let end_ln = LineNumber::new("end");
      let inner_ln = LineNumber::new("inner");
      items.push_condition(Condition::new(
        "fits",
        ConditionProperties {
          condition: arena.alloc_resolver(move |context| context.fits_on_line_until(end_ln).map(|_| true)),
          true_path: None,
          false_path: None,
        },
      ));
      items.push_str_runtime_width_computed("abcdef");
      items.push_signal(Signal::SpaceOrNewLine);
      items.push_info(inner_ln);
      items.push_str_runtime_width_computed("ghijkl");
      items.push_info(end_ln);
      items.push_condition(conditions::if_true(
        "innerOnSecondLine",
        arena.alloc_resolver(move |context| context.resolved_line_number(inner_ln).map(|line| line == 1)),
        " ok".into_print_items(arena),
      ));
      items
    },
    PrintOptions {
      indent_width: 2,
      max_width: 10,
      use_tabs: false,
      new_line_text: "\n",
    },
  );
  assert_eq!(text, "abcdef\nghijkl ok");
}

#[test]
fn keep_line_before_here_forgets_earlier_possible_newline() {
  let format_text = |keep_line: bool| {
    format(
      |arena| {
        let mut items = PrintItems::new(arena);
        items.push_str_runtime_width_computed("abc");
        items.push_signal(Signal::PossibleNewLine);
        items.push_str_runtime_width_computed("def");
        if keep_line {
          items.push_signal(Signal::KeepLineBeforeHere);
        }
        items.push_str_runtime_width_computed("ghijkl");
        items
      },
      PrintOptions {
        indent_width: 2,
        max_width: 10,
        use_tabs: false,
        new_line_text: "\n",
      },
    )
  };
  assert_eq!(format_text(false), "abc\ndefghijkl");
  assert_eq!(format_text(true), "abcdefghijkl");
}
