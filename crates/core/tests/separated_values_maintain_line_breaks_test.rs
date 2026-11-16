use dprint_core::formatting::PrintItems;
use dprint_core::formatting::PrintOptions;
use dprint_core::formatting::Signal;
use dprint_core::formatting::ir_helpers;

#[test]
fn should_maintain_line_breaks_between_values() {
  assert_eq!(print_values(&[0, 0, 1, 1], surround_newlines_options()), "[\n  a, b,\n  c, d,\n]");
  assert_eq!(print_values(&[0, 1, 1, 2], surround_newlines_options()), "[\n  a,\n  b, c,\n  d,\n]");
}

#[test]
fn should_keep_values_on_one_line_when_newline_at_start() {
  // the newline at the start is what makes it multi-line, so the last value stays on the same line
  assert_eq!(print_values(&[0, 0], surround_newlines_options()), "[\n  a, b,\n]");
  assert_eq!(print_values(&[0, 0, 0], surround_newlines_options()), "[\n  a, b, c,\n]");
}

#[test]
fn should_wrap_values_that_exceed_the_width() {
  assert_eq!(
    print_values(&[0, 0, 0, 0, 0, 0, 0, 1, 1], surround_newlines_options()),
    "[\n  a, b, c, d, e,\n  f, g,\n  h, i,\n]"
  );
}

#[test]
fn should_force_newline_for_last_value_when_no_newline_at_start() {
  // nothing else makes it multi-line, so the last value goes on a new line
  assert_eq!(print_values(&[0, 0, 0], ir_helpers::MultiLineOptions::maintain_line_breaks()), "[a, b,\nc,]");
}

#[test]
fn should_maintain_line_breaks_when_hanging() {
  let print = |start_lines: &[usize], multi_line_options| {
    print(PrintValuesOptions {
      values: start_lines.iter().map(|line| Value::single_line(*line)).collect(),
      multi_line_options,
      prefer_hanging: true,
      force_use_new_lines: false,
    })
  };
  assert_eq!(print(&[0, 0, 1, 1], surround_newlines_options()), "[a, b,\n  c, d,]");
  assert_eq!(print(&[0, 1, 1, 2], surround_newlines_options()), "[a,\n  b, c,\n  d,]");
  // fills the lines when there are no line breaks
  assert_eq!(print(&[0, 0, 0], surround_newlines_options()), "[a, b, c,]");
  assert_eq!(print(&[0, 0, 0, 0, 0, 0, 0], surround_newlines_options()), "[a, b, c, d, e,\n  f, g,]");
  // and when not maintaining the line breaks
  assert_eq!(print(&[0, 1, 2], ir_helpers::MultiLineOptions::surround_newlines_indented()), "[a, b, c,]");
}

#[test]
fn should_not_maintain_line_breaks_when_single_line_and_not_hanging() {
  assert_eq!(
    print(PrintValuesOptions {
      values: [0, 1, 2].into_iter().map(Value::single_line).collect(),
      multi_line_options: surround_newlines_options(),
      prefer_hanging: false,
      force_use_new_lines: false,
    }),
    "[a, b, c,]"
  );
}

#[test]
fn should_use_newline_after_value_made_multi_line() {
  let print = |second_value_end_line: usize, newline_after_value_made_multi_line: bool| {
    print(PrintValuesOptions {
      values: vec![
        Value::single_line(0),
        Value {
          start_line: 0,
          end_line: second_value_end_line,
          is_printed_multi_line: true,
        },
        Value::single_line(second_value_end_line),
      ],
      multi_line_options: ir_helpers::MultiLineOptions {
        newline_after_value_made_multi_line,
        ..surround_newlines_options()
      },
      prefer_hanging: false,
      force_use_new_lines: true,
    })
  };
  // the second value is on a single line in the source, but is printed on multiple lines
  assert_eq!(print(0, true), "[\n  a, b\n  b,\n  c,\n]");
  assert_eq!(print(0, false), "[\n  a, b\n  b, c,\n]");
  // the second value is on multiple lines in the source, so the line breaks are maintained
  assert_eq!(print(1, true), "[\n  a, b\n  b, c,\n]");
}

fn surround_newlines_options() -> ir_helpers::MultiLineOptions<'static> {
  ir_helpers::MultiLineOptions {
    maintain_line_breaks: true,
    ..ir_helpers::MultiLineOptions::surround_newlines_indented()
  }
}

/// Prints a value for each start line in multi-line mode, where each value spans a single line.
fn print_values(start_lines: &[usize], multi_line_options: ir_helpers::MultiLineOptions<'static>) -> String {
  print(PrintValuesOptions {
    values: start_lines.iter().map(|line| Value::single_line(*line)).collect(),
    multi_line_options,
    prefer_hanging: false,
    force_use_new_lines: true,
  })
}

struct PrintValuesOptions {
  values: Vec<Value>,
  multi_line_options: ir_helpers::MultiLineOptions<'static>,
  prefer_hanging: bool,
  force_use_new_lines: bool,
}

struct Value {
  start_line: usize,
  end_line: usize,
  /// Whether to print the value over two lines.
  is_printed_multi_line: bool,
}

impl Value {
  fn single_line(line: usize) -> Self {
    Value {
      start_line: line,
      end_line: line,
      is_printed_multi_line: false,
    }
  }
}

fn print(opts: PrintValuesOptions) -> String {
  dprint_core::formatting::format(
    |arena| {
      let group = ir_helpers::gen_separated_values(
        |_| {
          opts
            .values
            .iter()
            .enumerate()
            .map(|(i, value)| ir_helpers::GeneratedValue {
              items: {
                let text = (b'a' + i as u8) as char;
                let mut items = PrintItems::new(arena);
                if value.is_printed_multi_line {
                  items.push_string(text.to_string());
                  items.push_signal(Signal::NewLine);
                }
                items.push_string(format!("{},", text));
                items
              },
              lines_span: Some(ir_helpers::LinesSpan {
                start_line: value.start_line,
                end_line: value.end_line,
              }),
              allow_inline_multi_line: false,
              allow_inline_single_line: false,
              is_known_multi_line: false,
            })
            .collect::<Vec<_>>()
        },
        ir_helpers::GenSeparatedValuesOptions {
          prefer_hanging: opts.prefer_hanging,
          force_use_new_lines: opts.force_use_new_lines,
          max_blank_lines: 0,
          single_line_options: ir_helpers::SingleLineOptions::same_line_maybe_space_separated(arena),
          indent_width: 2,
          multi_line_options: opts.multi_line_options,
          force_possible_newline_at_start: false,
        },
      )
      .items;
      let mut items = PrintItems::new(arena);
      items.push_str("[");
      items.extend(group);
      items.push_str("]");
      items
    },
    PrintOptions {
      indent_width: 2,
      max_width: 16,
      use_tabs: false,
      new_line_text: "\n",
    },
  )
}
