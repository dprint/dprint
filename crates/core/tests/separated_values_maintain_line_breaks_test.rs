use dprint_core::formatting::PrintItems;
use dprint_core::formatting::PrintOptions;
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

fn surround_newlines_options() -> ir_helpers::MultiLineOptions<'static> {
  ir_helpers::MultiLineOptions {
    maintain_line_breaks: true,
    ..ir_helpers::MultiLineOptions::surround_newlines_indented()
  }
}

/// Prints a value for each start line in multi-line mode, where each value spans a single line.
fn print_values(start_lines: &[usize], multi_line_options: ir_helpers::MultiLineOptions<'static>) -> String {
  let start_lines = start_lines.to_vec();
  dprint_core::formatting::format(
    |arena| {
      let group = ir_helpers::gen_separated_values(
        |_| {
          start_lines
            .iter()
            .enumerate()
            .map(|(i, start_line)| ir_helpers::GeneratedValue {
              items: {
                let mut items = PrintItems::new(arena);
                items.push_string(format!("{},", (b'a' + i as u8) as char));
                items
              },
              lines_span: Some(ir_helpers::LinesSpan {
                start_line: *start_line,
                end_line: *start_line,
              }),
              allow_inline_multi_line: false,
              allow_inline_single_line: false,
              is_known_multi_line: false,
            })
            .collect::<Vec<_>>()
        },
        ir_helpers::GenSeparatedValuesOptions {
          prefer_hanging: false,
          force_use_new_lines: true,
          max_blank_lines: 0,
          single_line_options: ir_helpers::SingleLineOptions::same_line_maybe_space_separated(arena),
          indent_width: 2,
          multi_line_options,
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
