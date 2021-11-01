pub fn get_line_number_of_pos(text: &str, pos: usize) -> usize {
  let text_bytes = text.as_bytes();
  let mut line_count = 1; // 1-indexed

  for i in 0..pos {
    if text_bytes.get(i) == Some(&(b'\n')) {
      line_count += 1;
    }
  }

  line_count
}

pub fn get_column_number_of_pos(text: &str, pos: usize) -> usize {
  let line_start_byte_pos = get_line_start_byte_pos(text, pos);
  return text[line_start_byte_pos..pos].chars().count() + 1; // 1-indexed
}

fn get_line_start_byte_pos(text: &str, pos: usize) -> usize {
  let text_bytes = text.as_bytes();
  for i in (0..pos).rev() {
    if text_bytes.get(i) == Some(&(b'\n')) {
      return i + 1;
    }
  }

  0
}

fn get_line_end_byte_pos(text: &str, pos: usize) -> usize {
  let mut pos = pos;
  let mut chars = text[pos..].chars().peekable();
  while let Some(c) = chars.next() {
    if c == '\n' || c == '\r' && chars.peek().copied() == Some('\n') {
      break;
    }
    pos += c.len_utf8();
  }
  pos
}

pub fn format_diagnostic(range: Option<(usize, usize)>, message: &str, file_text: &str) -> String {
  let mut result = String::new();
  if let Some((error_start, _)) = range {
    let line_number = get_line_number_of_pos(file_text, error_start);
    let column_number = get_column_number_of_pos(file_text, error_start);
    result.push_str(&format!("Line {}, column {}: ", line_number, column_number))
  }
  result.push_str(message);
  if let Some(range) = range {
    result.push_str("\n\n");
    let code = get_range_text_highlight(file_text, range)
      .lines()
      .map(|l| format!("  {}", l)) // indent
      .collect::<Vec<_>>()
      .join("\n");
    result.push_str(&code);
  }
  result
}

fn get_range_text_highlight(file_text: &str, byte_range: (usize, usize)) -> String {
  // todo: cleanup... kind of confusing
  let ((text_start, text_end), (error_start, error_end)) = get_text_and_error_range(byte_range, file_text);
  if text_end > file_text.len() {
    return format!("Error formatting diagnostic. Position {} was outside the length of the string.", text_end);
  }
  let sub_text = &file_text[text_start..text_end];

  let mut result = String::new();
  let lines = sub_text.lines().collect::<Vec<_>>();
  let line_count = lines.len();
  for (i, line) in lines.iter().enumerate() {
    let is_last_line = i == line_count - 1;
    // don't show all the lines if there are more than 3 lines
    if i > 2 && !is_last_line {
      continue;
    }
    if i > 0 {
      result.push('\n');
    }
    if i == 2 && !is_last_line {
      result.push_str("...");
      continue;
    }
    result.push_str(line);
    result.push('\n');

    let start_char_index = if i == 0 { get_column_number_of_pos(sub_text, error_start) - 1 } else { 0 };
    let end_char_index = if is_last_line {
      get_column_number_of_pos(sub_text, error_end) - 1
    } else {
      line.chars().count()
    };
    result.push_str(&" ".repeat(start_char_index));
    result.push_str(&"~".repeat(end_char_index - start_char_index));
  }
  return result;

  fn get_text_and_error_range(byte_range: (usize, usize), file_text: &str) -> ((usize, usize), (usize, usize)) {
    let (start, end) = byte_range;
    let line_start = get_line_start_byte_pos(file_text, start);
    let line_end = get_line_end_byte_pos(file_text, end);

    let start_text = &file_text[line_start..start];
    let end_text = &file_text[end..line_end];

    let text_start = start - start_text.chars().rev().take(20).map(|c| c.len_utf8()).sum::<usize>();
    let text_end = end + end_text.chars().take(10).map(|c| c.len_utf8()).sum::<usize>();
    let error_start = start - text_start;
    let error_end = error_start + (end - start);

    ((text_start, text_end), (error_start, error_end))
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  // get_line_number_of_pos

  #[test]
  fn should_get_line_number_of_single_line() {
    assert_eq!(get_line_number_of_pos("testing", 3), 1);
  }

  #[test]
  fn should_get_last_line_when_above_length() {
    assert_eq!(get_line_number_of_pos("t\nt", 50), 2);
  }

  #[test]
  fn should_get_line_when_at_first_pos_on_line() {
    assert_eq!(get_line_number_of_pos("t\ntest\nt", 2), 2);
  }

  #[test]
  fn should_get_line_when_at_last_pos_on_line() {
    assert_eq!(get_line_number_of_pos("t\ntest\nt", 6), 2);
  }

  // get_column_number_of_pos

  #[test]
  fn should_get_column_for_first_line() {
    assert_eq!(get_column_number_of_pos("testing\nthis", 3), 4);
  }

  #[test]
  fn should_get_column_for_second_line() {
    assert_eq!(get_column_number_of_pos("test\nthis", 6), 2);
  }

  #[test]
  fn should_get_column_for_start_of_text() {
    assert_eq!(get_column_number_of_pos("test\nthis", 0), 1);
  }

  #[test]
  fn should_get_column_for_start_of_line() {
    assert_eq!(get_column_number_of_pos("test\nthis", 5), 1);
  }

  // get_range_text_highlight

  #[test]
  fn should_get_range_highlight_for_full_text_one_line() {
    let message = get_range_text_highlight("testtinga", (0, 9));
    assert_eq!(message, concat!("testtinga\n", "~~~~~~~~~"));
  }

  #[test]
  fn should_get_range_highlight_for_full_text_multi_lines() {
    let message = get_range_text_highlight("test\nt\naa", (0, 9));
    assert_eq!(message, concat!("test\n", "~~~~\n", "t\n", "~\n", "aa\n", "~~"));
  }

  #[test]
  fn should_get_range_highlight_on_one_line() {
    let message = get_range_text_highlight("testtinga testing test", (10, 17));
    assert_eq!(message, concat!("testtinga testing test\n", "          ~~~~~~~"));
  }

  #[test]
  fn should_get_range_highlight_on_second_line() {
    let message = get_range_text_highlight("test\ntest\ntest", (5, 9));
    assert_eq!(message, concat!("test\n", "~~~~"));
  }

  #[test]
  fn should_get_range_highlight_on_multi_lines_within() {
    let message = get_range_text_highlight("test\ntest test\ntest test\nasdf", (10, 19));
    assert_eq!(message, concat!("test test\n", "     ~~~~\n", "test test\n", "~~~~"));
  }

  #[test]
  fn should_display_when_there_are_three_lines() {
    let message = get_range_text_highlight("test\nasdf\n1234\ntest\nasdf\n1234\ntest\n", (5, 19));
    assert_eq!(message, concat!("asdf\n", "~~~~\n", "1234\n", "~~~~\n", "test\n", "~~~~"));
  }

  #[test]
  fn should_ignore_when_there_are_more_than_three_lines() {
    let message = get_range_text_highlight("test\nasdf\n1234\ntest\nasdf\n1234\ntest\n", (5, 24));
    assert_eq!(message, concat!("asdf\n", "~~~~\n", "1234\n", "~~~~\n", "...\n", "asdf\n", "~~~~"));
  }

  #[test]
  fn should_show_only_twenty_chars_of_first_line() {
    let message = get_range_text_highlight("test asdf 1234 fdsa dsfa test", (25, 29));
    assert_eq!(message, concat!("asdf 1234 fdsa dsfa test\n", "                    ~~~~",));
  }

  #[test]
  fn should_show_only_ten_chars_of_last_line() {
    let message = get_range_text_highlight("test asdf 1234 fdsa dsfa test", (10, 14));
    assert_eq!(message, concat!("test asdf 1234 fdsa dsfa\n", "          ~~~~",));
  }

  #[test]
  fn should_handle_multi_byte_chars() {
    let one_to_ten = "一二三四五六七八九十";
    let message = get_range_text_highlight(
      &one_to_ten.repeat(6),
      (one_to_ten.len() * 3, one_to_ten.len() * 3 + one_to_ten.chars().next().unwrap().len_utf8()),
    );
    assert_eq!(
      message,
      concat!("一二三四五六七八九十一二三四五六七八九十一二三四五六七八九十一\n", "                    ~",)
    );
  }

  #[test]
  fn should_handle_multi_byte_characters_on_the_first_line() {
    let text = "test ≥ ; test";
    let semi_colon_index = text.find(';').unwrap();
    let message = get_range_text_highlight(text, (semi_colon_index, semi_colon_index + 1));
    assert_eq!(message, concat!("test ≥ ; test\n", "       ~",));
  }

  #[test]
  fn should_handle_multi_byte_characters_on_the_second_line() {
    let text = "≥a\ntest ≥ ; test";
    let semi_colon_index = text.find(';').unwrap();
    let message = get_range_text_highlight(text, (semi_colon_index, semi_colon_index + 1));
    assert_eq!(message, concat!("test ≥ ; test\n", "       ~",));
  }
}
