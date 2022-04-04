use std::rc::Rc;

use crate::formatting::condition_helpers;

use super::super::conditions;
use super::super::print_items::*;

pub fn surround_with_new_lines(item: PrintItems) -> PrintItems {
  if item.is_empty() {
    return item;
  }

  let mut items = PrintItems::new();
  items.push_signal(Signal::NewLine);
  items.extend(item);
  items.push_signal(Signal::NewLine);
  items
}

pub fn with_indent(item: PrintItems) -> PrintItems {
  with_indent_times(item, 1)
}

pub fn with_queued_indent(item: PrintItems) -> PrintItems {
  if item.is_empty() {
    return item;
  }

  let mut items = PrintItems::new();
  items.push_signal(Signal::QueueStartIndent);
  items.extend(item);
  items.push_signal(Signal::FinishIndent);
  items
}

pub fn with_indent_times(item: PrintItems, times: u32) -> PrintItems {
  if item.is_empty() {
    return item;
  }

  let mut items = PrintItems::new();
  for _ in 0..times {
    items.push_signal(Signal::StartIndent);
  }
  items.extend(item);
  for _ in 0..times {
    items.push_signal(Signal::FinishIndent);
  }
  items
}

pub fn with_no_new_lines(item: PrintItems) -> PrintItems {
  if item.is_empty() {
    return item;
  }

  let mut items = PrintItems::new();
  items.push_signal(Signal::StartForceNoNewLines);
  items.extend(item);
  items.push_signal(Signal::FinishForceNoNewLines);
  items
}

pub fn new_line_group(item: PrintItems) -> PrintItems {
  if item.is_empty() {
    return item;
  }

  let mut items = PrintItems::new();
  items.push_signal(Signal::StartNewLineGroup);
  items.extend(item);
  items.push_signal(Signal::FinishNewLineGroup);
  items
}

/// Generates IR from a string as is and ignores its indent.
pub fn gen_from_raw_string(text: &str) -> PrintItems {
  gen_from_raw_string_lines(text, gen_from_string)
}

/// Generates IR from a string trimming the end of each line and ignores its indent.
pub fn gen_from_raw_string_trim_line_ends(text: &str) -> PrintItems {
  gen_from_raw_string_lines(text, |line_text| gen_from_string_line(line_text.trim_end()))
}

fn gen_from_raw_string_lines(text: &str, gen_line: impl Fn(&str) -> PrintItems) -> PrintItems {
  let add_ignore_indent = text.contains('\n');
  let mut items = PrintItems::new();
  if add_ignore_indent {
    items.push_signal(Signal::StartIgnoringIndent);
  }
  items.extend(gen_string_lines(text, gen_line));
  if add_ignore_indent {
    items.push_signal(Signal::FinishIgnoringIndent);
  }
  items
}

/// Generates IR from a string to a series of PrintItems.
pub fn gen_from_string(text: &str) -> PrintItems {
  gen_string_lines(text, gen_from_string_line)
}

/// Generates IR from a string to a series of PrintItems trimming the end of each line for whitespace.
pub fn gen_from_string_trim_line_ends(text: &str) -> PrintItems {
  gen_string_lines(text, |line_text| gen_from_string_line(line_text.trim_end()))
}

fn gen_string_lines(text: &str, gen_line: impl Fn(&str) -> PrintItems) -> PrintItems {
  let mut items = PrintItems::new();

  for (i, line) in text.lines().enumerate() {
    if i > 0 {
      items.push_signal(Signal::NewLine);
    }

    items.extend(gen_line(line));
  }

  // using .lines() will remove the last line, so add it back if it exists
  if text.ends_with('\n') {
    items.push_signal(Signal::NewLine)
  }

  items
}

fn gen_from_string_line(line: &str) -> PrintItems {
  let mut items = PrintItems::new();
  for (i, line) in line.split('\t').enumerate() {
    if i > 0 {
      items.push_signal(Signal::Tab);
    }
    if !line.is_empty() {
      items.push_str(line);
    }
  }
  items
}

/// Surrounds the items with newlines and indentation if its on multiple lines.
/// Note: This currently inserts a possible newline at the start, but that might change or be made
/// conditional in the future.
pub fn surround_with_newlines_indented_if_multi_line(inner_items: PrintItems, indent_width: u8) -> PrintItems {
  if inner_items.is_empty() {
    return inner_items;
  }

  let mut items = PrintItems::new();
  let start_ln = LineNumber::new("surroundWithNewLinesIndentedIfMultiLineStart");
  let end_ln = LineNumber::new("surroundWithNewLineIndentedsIfMultiLineEnd");
  let inner_items = inner_items.into_rc_path();

  items.push_line_number(start_ln);
  items.push_line_number_anchor(LineNumberAnchor::new("surroundWithNewLineIndentedsIfMultiLineEnd", end_ln));
  let mut condition = Condition::new(
    "newlineIfMultiLine",
    ConditionProperties {
      true_path: Some(surround_with_new_lines(with_indent(inner_items.into()))),
      false_path: Some({
        let mut items = PrintItems::new();
        items.push_condition(conditions::if_above_width(indent_width, Signal::PossibleNewLine.into()));
        items.extend(inner_items.into());
        items
      }),
      condition: Rc::new(move |context| condition_helpers::is_multiple_lines(context, start_ln, end_ln)),
    },
  );
  let condition_reevaluation = condition.create_reevaluation();
  items.push_condition(condition);
  items.push_line_number(end_ln);
  items.push_reevaluation(condition_reevaluation);

  items
}

/// Generates IR from the provided text to a JS-like comment line (ex. `// some text`)
pub fn gen_js_like_comment_line(text: &str, force_space_after_slashes: bool) -> PrintItems {
  let mut items = PrintItems::new();
  items.extend(gen_from_raw_string(&get_comment_text(text, force_space_after_slashes)));
  items.push_signal(Signal::ExpectNewLine);
  return with_no_new_lines(items);

  fn get_comment_text(original_text: &str, force_space_after_slashes: bool) -> String {
    let non_slash_index = get_first_non_slash_index(original_text);
    let skip_space = force_space_after_slashes && original_text.chars().nth(non_slash_index) == Some(' ');
    let start_text_index = if skip_space { non_slash_index + 1 } else { non_slash_index };
    let comment_text_original = &original_text[start_text_index..]; // pref: ok to index here since slashes are 1 byte each
    let comment_text = comment_text_original.trim_end();
    let prefix = format!("//{}", original_text.chars().take(non_slash_index).collect::<String>());

    return if comment_text.is_empty() {
      prefix
    } else {
      format!("{}{}{}", prefix, if force_space_after_slashes { " " } else { "" }, comment_text)
    };

    fn get_first_non_slash_index(text: &str) -> usize {
      let mut i: usize = 0;
      for c in text.chars() {
        if c != '/' {
          return i;
        }
        i += 1;
      }

      i
    }
  }
}

/// Generates IR from the provided text to a JS-like comment block (ex. `/** some text */`)
pub fn gen_js_like_comment_block(text: &str) -> PrintItems {
  let mut items = PrintItems::new();
  let add_ignore_indent = text.contains('\n');
  let last_line_trailing_whitespace = get_last_line_trailing_whitespace(text);

  items.push_str("/*");
  if add_ignore_indent {
    items.push_signal(Signal::StartIgnoringIndent);
  }
  items.extend(gen_from_string_trim_line_ends(text));

  // add back the last line's trailing whitespace
  if !last_line_trailing_whitespace.is_empty() {
    items.extend(gen_from_raw_string(last_line_trailing_whitespace));
  }

  if add_ignore_indent {
    items.push_signal(Signal::FinishIgnoringIndent);
  }
  items.push_str("*/");

  return items;

  fn get_last_line_trailing_whitespace(text: &str) -> &str {
    let end_text = &text[text.trim_end().len()..];
    if let Some(last_index) = end_text.rfind('\n') {
      &end_text[last_index + 1..]
    } else {
      end_text
    }
  }
}

/// Gets if the provided text has the provided searching text in it (ex. "dprint-ignore").
pub fn text_has_dprint_ignore(text: &str, searching_text: &str) -> bool {
  let pos = text.find(searching_text);
  if let Some(pos) = pos {
    let end = pos + searching_text.len();
    if pos > 0 && is_alpha_numeric_at_pos(text, pos - 1) {
      return false;
    }
    if is_alpha_numeric_at_pos(text, end) {
      return false;
    }
    return true;
  } else {
    return false;
  }

  fn is_alpha_numeric_at_pos(text: &str, pos: usize) -> bool {
    if let Some(chars_after) = text.get(pos..) {
      if let Some(char_after) = chars_after.chars().next() {
        return char_after.is_alphanumeric();
      }
    }
    false
  }
}
