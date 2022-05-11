use std::time::Duration;

use crossterm::style::Stylize;
use similar::ChangeTag;
use similar::TextDiffConfig;

/// Gets a string showing the difference between two strings.
pub fn get_difference(old_text: &str, new_text: &str) -> String {
  debug_assert!(old_text != new_text);

  // normalize newlines
  let old_text = old_text.replace("\r\n", "\n");
  let new_text = new_text.replace("\r\n", "\n");

  if old_text == new_text {
    return String::from(" | Text differed by line endings.");
  }

  let mut config = TextDiffConfig::default();
  config.timeout(Duration::from_millis(500));

  let diff = config.diff_lines(&old_text, &new_text);

  let mut output = String::new();
  for hunk in diff.unified_diff().iter_hunks() {
    if !output.is_empty() {
      output.push_str("\n...");
    }
    let max_old_line_num_width = get_number_char_count(hunk.iter_changes().filter_map(|c| c.old_index().map(|i| i + 1)));
    let max_new_line_num_width = get_number_char_count(hunk.iter_changes().filter_map(|c| c.new_index().map(|i| i + 1)));
    for op in hunk.ops() {
      for change in diff.iter_inline_changes(op) {
        if !output.is_empty() {
          output.push('\n');
        }
        let sign = match change.tag() {
          ChangeTag::Delete => get_removal_text("-"),
          ChangeTag::Insert => get_addition_text("+"),
          ChangeTag::Equal => " ".to_string(),
        };
        output.push_str(&format!(
          "{:>old_width$} {:>new_width$}|{}",
          change.new_index().map(|i| (i + 1).to_string()).unwrap_or_else(|| "".to_string()),
          change.old_index().map(|i| (i + 1).to_string()).unwrap_or_else(|| "".to_string()),
          sign,
          old_width = max_old_line_num_width,
          new_width = max_new_line_num_width,
        ));
        for (highlight, change_text) in change.iter_strings_lossy() {
          let change_text = if let Some(change_text) = change_text.strip_suffix("\r\n") {
            change_text
          } else if let Some(change_text) = change_text.strip_suffix('\n') {
            change_text
          } else {
            &change_text
          };
          let change_text = annotate_whitespace(change_text);
          if !change_text.is_empty() {
            let change_text = if highlight {
              match change.tag() {
                ChangeTag::Delete => get_removal_highlight_text(&change_text),
                ChangeTag::Insert => get_addition_highlight_text(&change_text),
                ChangeTag::Equal => change_text,
              }
            } else {
              get_text_for_tag(change.tag(), change_text)
            };
            output.push_str(&change_text);
          }
        }
        if change.missing_newline() {
          // show a ETX (end of text)
          output.push_str(&get_text_for_tag(change.tag(), "\u{2403}".to_string()));
        }
      }
    }
  }

  output
}

fn get_text_for_tag(tag: ChangeTag, text: String) -> String {
  match tag {
    ChangeTag::Delete => get_removal_text(&text),
    ChangeTag::Insert => get_addition_text(&text),
    ChangeTag::Equal => text,
  }
}

fn get_number_char_count(numbers: impl Iterator<Item = usize>) -> usize {
  numbers.max().unwrap_or(1).to_string().chars().count()
}

fn get_addition_text(text: &str) -> String {
  text.green().to_string()
}

fn get_addition_highlight_text(text: &str) -> String {
  let text = text.replace('\t', "\u{21E5}");
  text.black().on_green().to_string()
}

fn get_removal_text(text: &str) -> String {
  let text = text.replace('\t', "\u{21E5}");
  text.red().to_string()
}

fn get_removal_highlight_text(text: &str) -> String {
  let text = text.replace('\t', "\u{21E5}");
  text.white().on_red().to_string()
}

fn annotate_whitespace(text: &str) -> String {
  text.replace('\t', "\u{2192}").replace(' ', "\u{00B7}")
}

#[cfg(test)]
mod test {
  use super::*;
  use pretty_assertions::assert_eq;

  #[test]
  fn should_get_when_differs_by_line_endings() {
    assert_eq!(get_difference("test\r\n", "test\n"), " | Text differed by line endings.");
  }

  #[test]
  fn should_get_difference_on_one_line() {
    assert_eq!(
      get_difference("test1\n", "test2\n"),
      format!(
        "  1|{}{}\n1  |{}{}",
        get_removal_text("-"),
        get_removal_highlight_text("test1"),
        get_addition_text("+"),
        get_addition_highlight_text("test2"),
      )
    );
  }

  #[test]
  fn should_show_the_addition_of_last_line() {
    assert_eq!(
      get_difference("testing\ntesting", "testing\ntesting\n"),
      format!(
        "1 1| testing\n  2|{}{}{}\n2  |{}{}",
        get_removal_text("-"),
        get_removal_text("testing"),
        get_removal_text("\u{2403}"), // end of text
        get_addition_text("+"),
        get_addition_text("testing"),
      )
    );
  }

  #[test]
  fn should_get_difference_for_removed_line() {
    assert_eq!(
      get_difference("class Test\n{\n\n}", "class Test {\n}\n"),
      format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        format!("  1|{}{}", get_removal_text("-"), get_removal_text(&annotate_whitespace("class Test")),),
        format!("  2|{}{}", get_removal_text("-"), get_removal_text("{"),),
        format!("  3|{}", get_removal_text("-")),
        format!(
          "  4|{}{}{}",
          get_removal_text("-"),
          get_removal_text("}"),
          get_removal_text("\u{2403}"), // end of text
        ),
        format!(
          "1  |{}{}{}{}",
          get_addition_text("+"),
          get_addition_text(&annotate_whitespace("class Test")),
          get_addition_highlight_text(&annotate_whitespace(" ")),
          get_addition_text("{"),
        ),
        format!("2  |{}{}", get_addition_text("+"), get_addition_text("}"),),
      )
    );
  }

  #[test]
  fn should_show_multiple_removals_on_different_lines() {
    assert_eq!(
      get_difference("test ;\n1\n2\n3\n4\n5\n6\n7\n8\n9\ntest ;\n", "test;\n1\n2\n3\n4\n5\n6\n7\n8\n9\ntest;\n"),
      format!(
        "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
        format!("  1|{}{}", get_removal_text("-"), get_removal_text(&annotate_whitespace("test ;")),),
        format!("1  |{}{}", get_addition_text("+"), get_addition_text("test;"),),
        "2 2| 1",
        "3 3| 2",
        "4 4| 3",
        "...",
        " 8  8| 7",
        " 9  9| 8",
        "10 10| 9",
        format!("   11|{}{}", get_removal_text("-"), get_removal_text(&annotate_whitespace("test ;")),),
        format!("11   |{}{}", get_addition_text("+"), get_addition_text("test;"),),
      )
    );
  }
}
