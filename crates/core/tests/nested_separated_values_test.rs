use dprint_core::formatting::PrintItems;
use dprint_core::formatting::PrintOptions;
use dprint_core::formatting::ir_helpers;

/// Builds `depth` groups of separated values inside one another, each surrounded by brackets the way
/// a plugin writes an array, with the innermost group written over several lines.
///
/// Every group above the innermost one therefore has to be multi-line as well, which it only finds
/// out after printing its values.
fn print_nested(depth: usize) -> String {
  dprint_core::formatting::format(
    || {
      let mut inner = {
        let mut items = PrintItems::new();
        items.push_str("innermost");
        items
      };
      for i in 0..depth {
        let value = inner;
        let group = ir_helpers::gen_separated_values(
          |_| {
            vec![
              ir_helpers::GeneratedValue {
                items: value,
                lines_span: None,
                allow_inline_multi_line: false,
                allow_inline_single_line: false,
              },
              ir_helpers::GeneratedValue {
                items: {
                  let mut items = PrintItems::new();
                  items.push_str("second");
                  items
                },
                lines_span: None,
                allow_inline_multi_line: false,
                allow_inline_single_line: false,
              },
            ]
          },
          ir_helpers::GenSeparatedValuesOptions {
            prefer_hanging: false,
            // the innermost group is the one already written over several lines
            force_use_new_lines: i == 0,
            allow_blank_lines: false,
            single_line_options: ir_helpers::SingleLineOptions::separated_same_line(", ".into()),
            indent_width: 2,
            multi_line_options: ir_helpers::MultiLineOptions::surround_newlines_indented(),
            force_possible_newline_at_start: false,
          },
        )
        .items;
        inner = {
          let mut items = PrintItems::new();
          items.push_str("[");
          items.extend(group);
          items.push_str("]");
          items
        };
      }
      inner
    },
    PrintOptions {
      indent_width: 2,
      max_width: 80,
      use_tabs: false,
      new_line_text: "\n",
    },
  )
}

#[test]
fn should_print_nested_groups_over_multiple_lines() {
  // the separator is only used when a group fits on one line, so a caller that wants trailing
  // commas appends them to each value itself
  assert_eq!(print_nested(1), "[\n  innermost\n  second\n]");
  assert_eq!(
    print_nested(3),
    concat!(
      "[\n",
      "  [\n",
      "    [\n",
      "      innermost\n",
      "      second\n",
      "    ]\n",
      "    second\n",
      "  ]\n",
      "  second\n",
      "]",
    )
  );
}

/// Printing this is exponential in the nesting depth either way, so it is a question of how large
/// the base is. Each group assumes it fits on one line and only finds out otherwise once its values
/// have been printed, and finding out moves them.
///
/// Discarding what was already known about the values whenever they moved, rather than only when
/// they moved to a different column, doubled that base, because every group then printed its values
/// twice. Depth 24 took over 15 seconds in a release build and now takes under one.
///
/// Not run by default because it measures elapsed time, which depends on the machine and on whether
/// optimisations are on. Run it with `cargo test --release -- --ignored`.
#[test]
#[ignore = "measures elapsed time; run explicitly and in release"]
fn should_not_print_values_twice_per_level() {
  let start = std::time::Instant::now();
  let text = print_nested(24);
  let elapsed = start.elapsed();
  assert!(text.contains("innermost"));
  assert!(elapsed.as_secs() < 5, "took {elapsed:?}, so the base of the growth has gone up again");
}
