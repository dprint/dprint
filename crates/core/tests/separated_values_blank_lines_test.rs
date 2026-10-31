use dprint_core::formatting::PrintItems;
use dprint_core::formatting::PrintOptions;
use dprint_core::formatting::ir_helpers;

#[test]
fn should_keep_blank_lines_up_to_the_max() {
  // the values start on lines 0, 4 (three blank lines), and 6 (one blank line)
  let start_lines = [0, 4, 6];
  assert_eq!(print_values(&start_lines, 0), "[\n  a\n  b\n  c\n]");
  assert_eq!(print_values(&start_lines, 1), "[\n  a\n\n  b\n\n  c\n]");
  assert_eq!(print_values(&start_lines, 2), "[\n  a\n\n\n  b\n\n  c\n]");
  assert_eq!(print_values(&start_lines, 5), "[\n  a\n\n\n\n  b\n\n  c\n]");
}

#[test]
fn should_not_keep_blank_lines_when_single_line() {
  // the values fit on one line, so the blank lines between them don't apply
  assert_eq!(print_values_with_force_new_lines(&[0, 4], 2, false), "[a, b]");
}

fn print_values(start_lines: &[usize], max_blank_lines: u32) -> String {
  print_values_with_force_new_lines(start_lines, max_blank_lines, true)
}

/// Prints a value for each start line, where each value spans a single line.
fn print_values_with_force_new_lines(start_lines: &[usize], max_blank_lines: u32, force_use_new_lines: bool) -> String {
  let start_lines = start_lines.to_vec();
  dprint_core::formatting::format(
    || {
      let group = ir_helpers::gen_separated_values(
        |_| {
          start_lines
            .iter()
            .enumerate()
            .map(|(i, start_line)| ir_helpers::GeneratedValue {
              items: {
                let mut items = PrintItems::new();
                items.push_string(((b'a' + i as u8) as char).to_string());
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
            .collect()
        },
        ir_helpers::GenSeparatedValuesOptions {
          prefer_hanging: false,
          force_use_new_lines,
          max_blank_lines,
          single_line_options: ir_helpers::SingleLineOptions::separated_same_line(", ".into()),
          indent_width: 2,
          multi_line_options: ir_helpers::MultiLineOptions::surround_newlines_indented(),
          force_possible_newline_at_start: false,
        },
      )
      .items;
      let mut items = PrintItems::new();
      items.push_str("[");
      items.extend(group);
      items.push_str("]");
      items
    },
    PrintOptions {
      indent_width: 2,
      max_width: 80,
      use_tabs: false,
      new_line_text: "\n",
    },
  )
}
