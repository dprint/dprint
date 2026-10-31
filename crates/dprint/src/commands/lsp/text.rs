// Copyright 2018-2023 the Deno authors. All rights reserved. MIT license.

// The following code is lifted from Deno's codebase.
// https://github.com/denoland/deno/blob/35f028daf27bb40e86829e7b7cc19aa72a62c0a0/cli/lsp/text.rs

use anyhow::Result;
use anyhow::bail;
use deno_tower_lsp::lsp_types as lsp;
use deno_tower_lsp::lsp_types::TextEdit;
use dprint_core::plugins::FormatRange;
use similar::ChangeTag;
use similar::TextDiff;
use std::collections::HashMap;
use std::time::Duration;
use std::time::Instant;
use text_size::TextRange;
use text_size::TextSize;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Utf16Char {
  pub start: TextSize,
  pub end: TextSize,
}

impl Utf16Char {
  fn len(&self) -> TextSize {
    self.end - self.start
  }

  fn len_utf16(&self) -> usize {
    if self.len() == TextSize::from(4) { 2 } else { 1 }
  }
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct LineIndex {
  utf8_offsets: Vec<TextSize>,
  /// The u8 offset of the end of each line excluding its line ending.
  utf8_line_ends: Vec<TextSize>,
  utf16_lines: HashMap<u32, Vec<Utf16Char>>,
  utf16_offsets: Vec<TextSize>,
}

impl LineIndex {
  pub fn new(text: &str) -> LineIndex {
    let mut utf16_lines = HashMap::new();
    let mut utf16_chars = Vec::new();

    let mut utf8_offsets = vec![0.into()];
    let mut utf8_line_ends = Vec::new();
    let mut utf16_offsets = vec![0.into()];
    let mut was_last_cr = false;
    let mut curr_row = 0.into();
    let mut curr_col = 0.into();
    let mut curr_offset_u16 = 0.into();
    let mut line = 0;
    for c in text.chars() {
      let c_len = TextSize::of(c);
      curr_row += c_len;
      curr_offset_u16 += TextSize::from(c.len_utf16() as u32);
      if c == '\n' {
        let line_ending_len = if was_last_cr { 2 } else { 1 };
        utf8_line_ends.push(curr_row - TextSize::from(line_ending_len));
        was_last_cr = false;
        utf8_offsets.push(curr_row);
        utf16_offsets.push(curr_offset_u16);

        if !utf16_chars.is_empty() {
          utf16_lines.insert(line, utf16_chars);
          utf16_chars = Vec::new();
        }

        curr_col = 0.into();
        line += 1;
        continue;
      }
      was_last_cr = c == '\r';

      if !c.is_ascii() {
        utf16_chars.push(Utf16Char {
          start: curr_col,
          end: curr_col + c_len,
        });
      }
      curr_col += c_len;
    }

    // utf8_offsets and utf16_offsets length is equal to (# of lines + 1)
    utf8_offsets.push(curr_row);
    utf8_line_ends.push(curr_row);
    utf16_offsets.push(curr_offset_u16);

    if !utf16_chars.is_empty() {
      utf16_lines.insert(line, utf16_chars);
    }

    LineIndex {
      utf8_offsets,
      utf8_line_ends,
      utf16_lines,
      utf16_offsets,
    }
  }

  /// Convert a u16 based range to a u8 TextRange.
  pub fn get_text_range(&self, range: lsp::Range) -> Result<TextRange> {
    let start = self.offset(range.start);
    let end = self.offset(range.end);
    if start > end {
      bail!("The start of the range was after its end.")
    }
    Ok(TextRange::new(start, end))
  }

  /// Return a u8 offset based on a u16 position.
  ///
  /// A position past the end of a line is the end of that line, which is how
  /// the language server protocol defines it, and a position past the last
  /// line is the end of the text.
  pub fn offset(&self, position: lsp::Position) -> TextSize {
    let line = position.line as usize;
    let Some(line_end) = self.utf8_line_ends.get(line).copied() else {
      return self.utf8_line_ends.last().copied().unwrap_or_default();
    };
    let line_start = u32::from(self.utf8_offsets[line]);
    let col = u32::from(self.utf16_to_utf8_col(position.line, position.character));
    std::cmp::min(TextSize::from(line_start.saturating_add(col)), line_end)
  }

  /// Returns a u16 offset based on a u16 position, which is clamped like `offset`.
  pub fn offset_utf16(&self, position: lsp::Position) -> u32 {
    let offset = self.offset(position);
    let line = partition_point(&self.utf8_offsets, |&it| it <= offset) - 1;
    let col = offset - self.utf8_offsets[line];
    u32::from(self.utf16_offsets[line]) + self.utf8_to_utf16_col(line as u32, col)
  }

  /// Returns a u16 position based on a u16 offset.
  pub fn position_utf16(&self, offset: TextSize) -> lsp::Position {
    let line = partition_point(&self.utf16_offsets, |&it| it <= offset) - 1;
    let line_start_offset = self.utf16_offsets[line];
    let col = offset - line_start_offset;

    lsp::Position {
      line: line as u32,
      character: col.into(),
    }
  }

  /// Returns a u16 position based on a u8 offset.
  ///
  /// An offset in the middle of a character is the character's start.
  pub fn position_utf16_from_utf8_offset(&self, offset: TextSize) -> lsp::Position {
    let line = partition_point(&self.utf8_offsets, |&it| it <= offset) - 1;
    let col = offset - self.utf8_offsets[line];

    lsp::Position {
      line: line as u32,
      character: self.utf8_to_utf16_col(line as u32, col),
    }
  }

  /// Returns the zero based number of the last line, which is the line
  /// that the end of the text is on.
  pub fn last_line(&self) -> u32 {
    // only a default index has no lines
    self.utf8_line_ends.len().saturating_sub(1) as u32
  }

  fn utf16_to_utf8_col(&self, line: u32, mut col: u32) -> TextSize {
    if let Some(utf16_chars) = self.utf16_lines.get(&line) {
      for c in utf16_chars {
        if col > u32::from(c.start) {
          col = col.saturating_add(u32::from(c.len()) - c.len_utf16() as u32);
          // a position in the middle of a character (between the two
          // utf-16 code units of a surrogate pair) is the character's start
          if col < u32::from(c.end) {
            col = c.start.into();
            break;
          }
        } else {
          break;
        }
      }
    }

    col.into()
  }

  fn utf8_to_utf16_col(&self, line: u32, col: TextSize) -> u32 {
    let mut utf16_col = u32::from(col);
    if let Some(utf16_chars) = self.utf16_lines.get(&line) {
      for c in utf16_chars {
        if col >= c.end {
          utf16_col -= u32::from(c.len()) - c.len_utf16() as u32;
        } else {
          if col > c.start {
            utf16_col -= u32::from(col - c.start);
          }
          break;
        }
      }
    }
    utf16_col
  }
}

/// Normalizes the line endings of the formatted text to match those of the
/// source text.
///
/// Editors own the line endings of a document, so the language server should
/// not change them based on the plugin's configured newline kind. Doing so
/// produced spurious edits that some editors mishandled, resulting in doubled
/// or garbled newlines (see https://github.com/dprint/dprint/issues/965).
pub fn normalize_to_source_line_endings(source: &str, formatted: String) -> String {
  let Some(source_uses_crlf) = detect_crlf(source) else {
    return formatted; // can't tell what the source uses, so leave it alone
  };
  if Some(source_uses_crlf) == detect_crlf(&formatted) {
    return formatted;
  }
  if source_uses_crlf {
    formatted.replace("\r\n", "\n").replace('\n', "\r\n")
  } else {
    formatted.replace("\r\n", "\n")
  }
}

/// Determines whether the text uses `\r\n` line endings based on the first line
/// ending found. Returns `None` when there are no line endings.
fn detect_crlf(text: &str) -> Option<bool> {
  match text.find('\n') {
    None => None,
    Some(0) => Some(false),
    Some(index) => Some(text.as_bytes()[index - 1] == b'\r'),
  }
}

/// An edit of a text where the offsets are UTF-16 code units of that text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Utf16TextEdit {
  pub start: u32,
  pub end: u32,
  pub new_text: String,
}

/// Compare two strings and return the edits that change the first into the
/// second, which never split a character.
pub fn get_utf16_edits(a: &str, b: &str) -> Vec<Utf16TextEdit> {
  if a == b {
    return vec![];
  }
  // Heuristic to detect things like minified files. Diffing is expensive.
  // This is three times the number of line offsets of a line index of `a`.
  if b.chars().filter(|c| *c == '\n').count() > (a.chars().filter(|c| *c == '\n').count() + 2) * 3 {
    return vec![replace_whole_file(a, b)];
  }
  // diff at the character level, coalescing consecutive changes of the same
  // kind into runs so the edit construction below can work a chunk at a time.
  // Within a replacement similar always yields the deletions before the insertions.
  //
  // A deadline bounds pathological inputs. similar bails out and returns a valid
  // (but non-minimal) diff when it's hit, which would produce a large pile of
  // scattered edits, so in that case fall back to replacing the whole file.
  let deadline = Instant::now() + Duration::from_millis(500);
  let diff = TextDiff::configure().deadline(deadline).diff_chars(a, b);
  if Instant::now() >= deadline {
    return vec![replace_whole_file(a, b)];
  }
  let mut chunks: Vec<(ChangeTag, String)> = Vec::new();
  for change in diff.iter_all_changes() {
    match chunks.last_mut() {
      Some((tag, text)) if *tag == change.tag() => text.push_str(change.value()),
      _ => chunks.push((change.tag(), change.value().to_string())),
    }
  }

  let mut text_edits = Vec::<Utf16TextEdit>::new();
  let mut iter = chunks.into_iter().peekable();
  let mut a_pos = 0;
  loop {
    let chunk = iter.next();
    match chunk {
      None => break,
      Some((ChangeTag::Equal, e)) => {
        a_pos += e.encode_utf16().count() as u32;
      }
      Some((ChangeTag::Delete, d)) => {
        let start = a_pos;
        a_pos += d.encode_utf16().count() as u32;
        let end = a_pos;
        let new_text = match iter.next_if(|(tag, _)| *tag == ChangeTag::Insert) {
          Some((_, i)) => i,
          None => String::new(),
        };
        text_edits.push(Utf16TextEdit { start, end, new_text });
      }
      Some((ChangeTag::Insert, i)) => {
        text_edits.push(Utf16TextEdit {
          start: a_pos,
          end: a_pos,
          new_text: i,
        });
      }
    }
  }

  text_edits
}

/// A single edit that replaces the entire document with `new_text`.
fn replace_whole_file(old_text: &str, new_text: &str) -> Utf16TextEdit {
  Utf16TextEdit {
    start: 0,
    end: old_text.encode_utf16().count() as u32,
    new_text: new_text.to_string(),
  }
}

/// How to format the range of a text in an editor.
pub struct RangeFormat {
  /// The range to give the plugins, which is `None` for the whole text.
  pub plugin_range: FormatRange,
  /// The range of the edits to keep, which is `None` for all of them.
  pub edits_range: FormatRange,
}

impl RangeFormat {
  /// Gets how to format the range. Plugins that say they format a range are
  /// given it as it is because they know what to format around it (ex. the
  /// nodes it overlaps). The ones that don't say are given the range's whole
  /// lines and only the edits within those lines are kept.
  pub fn new(text: &str, range: FormatRange, plugins_format_range: bool) -> Self {
    if plugins_format_range {
      RangeFormat {
        plugin_range: range,
        edits_range: None,
      }
    } else {
      let range = range.and_then(|range| expand_to_lines(text, range));
      RangeFormat {
        plugin_range: range.clone(),
        edits_range: range,
      }
    }
  }

  /// The range a notebook cell is formatted with, which decides whether the
  /// cell's trailing whitespace is kept.
  pub fn cell_range(&self) -> FormatRange {
    // the edits are only kept within a range that the plugins are given too
    self.plugin_range.clone()
  }
}

/// Gets the edits that change the text to the formatted text, which are only
/// the ones within the range when there is one.
pub fn get_range_utf16_edits(text: &str, formatted_text: &str, range: &FormatRange) -> Vec<Utf16TextEdit> {
  let edits = get_utf16_edits(text, formatted_text);
  let Some(range) = range else {
    return edits;
  };
  let start = text[..range.start].encode_utf16().count() as u32;
  let end = start + text[range.clone()].encode_utf16().count() as u32;
  // an insertion after the range's last line break is on the next line
  let ends_with_line_break = text[range.clone()].ends_with('\n');
  edits
    .into_iter()
    .filter(|edit| edit.start >= start && edit.end <= end && !(ends_with_line_break && edit.start == end))
    .collect()
}

/// Applies the edits, which are in order and don't overlap, to the text.
pub fn apply_utf16_edits(text: &str, edits: &[Utf16TextEdit]) -> String {
  let mut result = String::with_capacity(text.len());
  let mut chars = text.chars();
  let mut offset = 0;
  for edit in edits {
    // the edits never split a character
    while offset < edit.start {
      let Some(c) = chars.next() else { break };
      result.push(c);
      offset += c.len_utf16() as u32;
    }
    while offset < edit.end {
      let Some(c) = chars.next() else { break };
      offset += c.len_utf16() as u32;
    }
    result.push_str(&edit.new_text);
  }
  result.extend(chars);
  result
}

/// Gets where the range of UTF-16 offsets is in the text once the edits are
/// applied to it. An edit that overlaps the range's start or end moves it to
/// the start or end of the edit's text, so the range has all of it.
pub fn map_utf16_range(range: std::ops::Range<u32>, edits: &[Utf16TextEdit]) -> std::ops::Range<u32> {
  let map = |offset: u32, is_end: bool| {
    let mut delta = 0i64;
    for edit in edits {
      let new_len = edit.new_text.encode_utf16().count() as i64;
      if edit.end <= offset {
        delta += new_len - (edit.end - edit.start) as i64;
      } else if edit.start < offset {
        // the offset is within the replaced text
        let mapped = edit.start as i64 + delta;
        return (if is_end { mapped + new_len } else { mapped }) as u32;
      } else {
        break;
      }
    }
    (offset as i64 + delta) as u32
  };
  let start = map(range.start, false);
  let end = std::cmp::max(start, map(range.end, true));
  start..end
}

/// Converts the edits of UTF-16 offsets to the edits of positions, which are
/// supported by the Language Server Protocol.
pub fn to_lsp_edits(edits: Vec<Utf16TextEdit>, line_index: &LineIndex) -> Vec<TextEdit> {
  edits.into_iter().map(|edit| to_lsp_edit(edit, line_index)).collect()
}

fn to_lsp_edit(edit: Utf16TextEdit, line_index: &LineIndex) -> TextEdit {
  TextEdit {
    range: lsp::Range {
      start: line_index.position_utf16(TextSize::from(edit.start)),
      end: line_index.position_utf16(TextSize::from(edit.end)),
    },
    new_text: edit.new_text,
  }
}

/// Expands the range to the whole lines it touches, including the last
/// line's line break, which is `None` for the whole text. A range that ends
/// at the start of a line (ex. after selecting whole lines) doesn't include
/// that line.
fn expand_to_lines(text: &str, range: std::ops::Range<usize>) -> FormatRange {
  let start = text[..range.start].rfind('\n').map(|index| index + 1).unwrap_or(0);
  let end = if range.end > start && text.as_bytes()[range.end - 1] == b'\n' {
    range.end
  } else {
    text[range.end..].find('\n').map(|index| range.end + index + 1).unwrap_or(text.len())
  };
  if start == 0 && end == text.len() { None } else { Some(start..end) }
}

fn partition_point<T, P>(slice: &[T], mut predicate: P) -> usize
where
  P: FnMut(&T) -> bool,
{
  let mut left = 0;
  let mut right = slice.len() - 1;

  while left != right {
    let mid = left + (right - left) / 2;
    // SAFETY:
    // When left < right, left <= mid < right.
    // Therefore left always increases and right always decreases,
    // and either of them is selected.
    // In both cases left <= right is satisfied.
    // Therefore if left < right in a step,
    // left <= right is satisfied in the next step.
    // Therefore as long as left != right, 0 <= left < right < len is satisfied
    // and if this case 0 <= mid < len is satisfied too.
    let value = unsafe { slice.get_unchecked(mid) };
    if predicate(value) {
      left = mid + 1;
    } else {
      right = mid;
    }
  }

  left
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_line_index() {
    let text = "hello\nworld";
    let index = LineIndex::new(text);
    assert_eq!(index.position_utf16(0.into()), lsp::Position { line: 0, character: 0 });
    assert_eq!(index.position_utf16(1.into()), lsp::Position { line: 0, character: 1 });
    assert_eq!(index.position_utf16(5.into()), lsp::Position { line: 0, character: 5 });
    assert_eq!(index.position_utf16(6.into()), lsp::Position { line: 1, character: 0 });
    assert_eq!(index.position_utf16(7.into()), lsp::Position { line: 1, character: 1 });
    assert_eq!(index.position_utf16(8.into()), lsp::Position { line: 1, character: 2 });
    assert_eq!(index.position_utf16(10.into()), lsp::Position { line: 1, character: 4 });
    assert_eq!(index.position_utf16(11.into()), lsp::Position { line: 1, character: 5 });
    assert_eq!(index.position_utf16(12.into()), lsp::Position { line: 1, character: 6 });

    let text = "\nhello\nworld";
    let index = LineIndex::new(text);
    assert_eq!(index.position_utf16(0.into()), lsp::Position { line: 0, character: 0 });
    assert_eq!(index.position_utf16(1.into()), lsp::Position { line: 1, character: 0 });
    assert_eq!(index.position_utf16(2.into()), lsp::Position { line: 1, character: 1 });
    assert_eq!(index.position_utf16(6.into()), lsp::Position { line: 1, character: 5 });
    assert_eq!(index.position_utf16(7.into()), lsp::Position { line: 2, character: 0 });
  }

  #[test]
  fn test_position_utf16_from_utf8_offset() {
    fn position(index: &LineIndex, offset: u32) -> (u32, u32) {
      let position = index.position_utf16_from_utf8_offset(offset.into());
      (position.line, position.character)
    }

    let index = LineIndex::new("hello\nworld");
    assert_eq!(position(&index, 0), (0, 0));
    assert_eq!(position(&index, 5), (0, 5));
    assert_eq!(position(&index, 6), (1, 0));
    assert_eq!(position(&index, 11), (1, 5));

    // é is two utf-8 bytes and one utf-16 code unit, 🦕 is four and two
    let index = LineIndex::new("a\u{e9}b\u{1F995}c\r\n\u{1F995}\u{e9}\n");
    assert_eq!(position(&index, 0), (0, 0));
    assert_eq!(position(&index, 1), (0, 1));
    assert_eq!(position(&index, 3), (0, 2));
    assert_eq!(position(&index, 4), (0, 3));
    assert_eq!(position(&index, 8), (0, 5));
    assert_eq!(position(&index, 9), (0, 6));
    assert_eq!(position(&index, 10), (0, 7));
    assert_eq!(position(&index, 11), (1, 0));
    assert_eq!(position(&index, 15), (1, 2));
    assert_eq!(position(&index, 17), (1, 3));
    assert_eq!(position(&index, 18), (2, 0));
    // in the middle of a character is the start of the character
    assert_eq!(position(&index, 2), (0, 1));
    assert_eq!(position(&index, 5), (0, 3));
    assert_eq!(position(&index, 7), (0, 3));
    assert_eq!(position(&index, 13), (1, 0));

    // round trips with the conversion in the other direction
    let text = "a\u{e9}b\u{1F995}c\n\u{1F995}\u{e9}\n\u{30e1} \u{30e1}";
    let index = LineIndex::new(text);
    for (offset, _) in text.char_indices().chain([(text.len(), ' ')]) {
      let offset = TextSize::from(offset as u32);
      assert_eq!(index.offset(index.position_utf16_from_utf8_offset(offset)), offset);
    }

    assert_eq!(position(&LineIndex::new(""), 0), (0, 0));
  }

  #[test]
  fn test_offset_out_of_range() {
    fn offset(index: &LineIndex, line: u32, character: u32) -> u32 {
      index.offset(lsp::Position { line, character }).into()
    }

    let index = LineIndex::new("ab\ncd");
    assert_eq!(offset(&index, 0, 2), 2);
    // past the end of a line is the end of the line
    assert_eq!(offset(&index, 0, 3), 2);
    assert_eq!(offset(&index, 0, 100), 2);
    assert_eq!(offset(&index, 0, u32::MAX), 2);
    assert_eq!(offset(&index, 1, 100), 5);
    // past the last line is the end of the text
    assert_eq!(offset(&index, 2, 0), 5);
    assert_eq!(offset(&index, 100, 100), 5);

    // excludes the line ending
    let index = LineIndex::new("ab\r\ncd\n");
    assert_eq!(offset(&index, 0, 100), 2);
    assert_eq!(offset(&index, 1, 100), 6);
    assert_eq!(offset(&index, 2, 0), 7);
    assert_eq!(offset(&index, 2, 100), 7);
    assert_eq!(offset(&index, 3, 0), 7);

    // empty text
    let index = LineIndex::new("");
    assert_eq!(offset(&index, 0, 0), 0);
    assert_eq!(offset(&index, 0, 5), 0);
    assert_eq!(offset(&index, 5, 5), 0);

    // multi-byte characters
    let index = LineIndex::new("a\u{1F995}b\n\u{e9}");
    assert_eq!(offset(&index, 0, 1), 1);
    // in the middle of the surrogate pair is the start of the character
    assert_eq!(offset(&index, 0, 2), 1);
    assert_eq!(offset(&index, 0, 3), 5);
    assert_eq!(offset(&index, 0, 4), 6);
    assert_eq!(offset(&index, 0, 100), 6);
    assert_eq!(offset(&index, 1, 1), 9);
    assert_eq!(offset(&index, 1, 100), 9);
  }

  #[test]
  fn test_last_line() {
    assert_eq!(LineIndex::new("").last_line(), 0);
    assert_eq!(LineIndex::new("ab").last_line(), 0);
    assert_eq!(LineIndex::new("ab\ncd").last_line(), 1);
    assert_eq!(LineIndex::new("ab\r\ncd\n").last_line(), 2);
    assert_eq!(LineIndex::default().last_line(), 0);
  }

  #[test]
  fn test_get_text_range_out_of_range() {
    fn range(index: &LineIndex, start: (u32, u32), end: (u32, u32)) -> Result<(u32, u32)> {
      let range = index.get_text_range(lsp::Range {
        start: lsp::Position {
          line: start.0,
          character: start.1,
        },
        end: lsp::Position { line: end.0, character: end.1 },
      })?;
      Ok((range.start().into(), range.end().into()))
    }

    let index = LineIndex::new("ab\ncd");
    // the start is past the end of its line, which was previously after the end
    assert_eq!(range(&index, (0, 100), (1, 0)).unwrap(), (2, 3));
    assert_eq!(range(&index, (0, 0), (0, 10)).unwrap(), (0, 2));
    assert_eq!(range(&index, (0, 0), (100, 0)).unwrap(), (0, 5));
    // start after the end
    assert!(range(&index, (0, 2), (0, 1)).is_err());
    assert!(range(&index, (1, 0), (0, 0)).is_err());
  }

  #[test]
  fn test_char_len() {
    assert_eq!('メ'.len_utf8(), 3);
    assert_eq!('メ'.len_utf16(), 1);
    assert_eq!('编'.len_utf8(), 3);
    assert_eq!('编'.len_utf16(), 1);
    assert_eq!('🦕'.len_utf8(), 4);
    assert_eq!('🦕'.len_utf16(), 2);
  }

  #[test]
  fn test_empty_index() {
    let col_index = LineIndex::new(
      "
const C: char = 'x';
",
    );
    assert_eq!(col_index.utf16_lines.len(), 0);
  }

  #[test]
  fn test_single_char() {
    let col_index = LineIndex::new(
      "
const C: char = 'メ';
",
    );

    assert_eq!(col_index.utf16_lines.len(), 1);
    assert_eq!(col_index.utf16_lines[&1].len(), 1);
    assert_eq!(
      col_index.utf16_lines[&1][0],
      Utf16Char {
        start: 17.into(),
        end: 20.into()
      }
    );

    // UTF-16 to UTF-8, no changes
    assert_eq!(col_index.utf16_to_utf8_col(1, 15), TextSize::from(15));

    // UTF-16 to UTF-8
    assert_eq!(col_index.utf16_to_utf8_col(1, 19), TextSize::from(21));

    let col_index = LineIndex::new("a𐐏b");
    assert_eq!(col_index.utf16_to_utf8_col(0, 3), TextSize::from(5));
  }

  #[test]
  fn test_string() {
    let col_index = LineIndex::new(
      "
const C: char = \"メ メ\";
",
    );

    assert_eq!(col_index.utf16_lines.len(), 1);
    assert_eq!(col_index.utf16_lines[&1].len(), 2);
    assert_eq!(
      col_index.utf16_lines[&1][0],
      Utf16Char {
        start: 17.into(),
        end: 20.into()
      }
    );
    assert_eq!(
      col_index.utf16_lines[&1][1],
      Utf16Char {
        start: 21.into(),
        end: 24.into()
      }
    );

    // UTF-16 to UTF-8
    assert_eq!(col_index.utf16_to_utf8_col(1, 15), TextSize::from(15));

    // メ UTF-8: 0xE3 0x83 0xA1, UTF-16: 0x30E1
    assert_eq!(col_index.utf16_to_utf8_col(1, 17), TextSize::from(17)); // first メ at 17..20
    assert_eq!(col_index.utf16_to_utf8_col(1, 18), TextSize::from(20)); // space
    assert_eq!(col_index.utf16_to_utf8_col(1, 19), TextSize::from(21)); // second メ at 21..24

    assert_eq!(col_index.utf16_to_utf8_col(2, 15), TextSize::from(15));
  }

  #[test]
  fn test_get_edits() {
    let a = "abcdefg";
    let b = "a\nb\nchije\nfg\n";
    let actual = to_lsp_edits(get_range_utf16_edits(a, b, &None), &LineIndex::new(a));
    assert_eq!(
      actual,
      vec![
        TextEdit {
          range: lsp::Range {
            start: lsp::Position { line: 0, character: 1 },
            end: lsp::Position { line: 0, character: 1 }
          },
          new_text: "\n".to_string()
        },
        TextEdit {
          range: lsp::Range {
            start: lsp::Position { line: 0, character: 2 },
            end: lsp::Position { line: 0, character: 2 }
          },
          new_text: "\n".to_string()
        },
        TextEdit {
          range: lsp::Range {
            start: lsp::Position { line: 0, character: 3 },
            end: lsp::Position { line: 0, character: 4 }
          },
          new_text: "hij".to_string()
        },
        TextEdit {
          range: lsp::Range {
            start: lsp::Position { line: 0, character: 5 },
            end: lsp::Position { line: 0, character: 5 }
          },
          new_text: "\n".to_string()
        },
        TextEdit {
          range: lsp::Range {
            start: lsp::Position { line: 0, character: 7 },
            end: lsp::Position { line: 0, character: 7 }
          },
          new_text: "\n".to_string()
        },
      ]
    );
  }

  #[test]
  fn test_replace_whole_file_non_ascii() {
    fn end(old_text: &str) -> lsp::Position {
      let edit = to_lsp_edit(replace_whole_file(old_text, "new"), &LineIndex::new(old_text));
      assert_eq!(edit.range.start, lsp::Position { line: 0, character: 0 });
      assert_eq!(edit.new_text, "new");
      edit.range.end
    }

    assert_eq!(end("ab\ncd"), lsp::Position { line: 1, character: 2 });
    assert_eq!(end("ab\ncd\n"), lsp::Position { line: 2, character: 0 });
    // é is two utf-8 bytes and one utf-16 code unit, 🦕 is four and two
    assert_eq!(end("\u{e9}\nab"), lsp::Position { line: 1, character: 2 });
    assert_eq!(end("a\u{e9}\u{1F995}"), lsp::Position { line: 0, character: 4 });
    assert_eq!(end("\u{e9}\u{e9}\u{e9}\n"), lsp::Position { line: 1, character: 0 });
    assert_eq!(end("\u{1F995}\r\n\u{e9}b\u{1F995}"), lsp::Position { line: 1, character: 4 });
  }

  #[test]
  fn test_normalize_to_source_line_endings() {
    // source LF, formatted CRLF -> formatted becomes LF
    assert_eq!(normalize_to_source_line_endings("a\nb\n", "a\r\nb\r\nc\r\n".to_string()), "a\nb\nc\n");
    // source CRLF, formatted LF -> formatted becomes CRLF
    assert_eq!(normalize_to_source_line_endings("a\r\nb\r\n", "a\nb\nc\n".to_string()), "a\r\nb\r\nc\r\n");
    // matching line endings are left untouched (no allocation churn issues)
    assert_eq!(normalize_to_source_line_endings("a\nb\n", "a\nb\n".to_string()), "a\nb\n");
    assert_eq!(normalize_to_source_line_endings("a\r\nb\r\n", "a\r\nb\r\n".to_string()), "a\r\nb\r\n");
    // no line endings in source -> leave formatted as-is
    assert_eq!(normalize_to_source_line_endings("abc", "abc\r\n".to_string()), "abc\r\n");
    // leading newline still detects the source kind
    assert_eq!(normalize_to_source_line_endings("\nabc", "\r\nabc".to_string()), "\nabc");
  }

  #[test]
  fn test_get_edits_after_normalizing_line_endings() {
    // when only the line endings differ, normalizing should produce no edits (#965)
    let a = "line1\nline2\nline3\n";
    let formatted = "line1\r\nline2\r\nline3\r\n".to_string();
    let new_text = normalize_to_source_line_endings(a, formatted);
    assert_eq!(to_lsp_edits(get_range_utf16_edits(a, &new_text, &None), &LineIndex::new(a)), vec![]);
  }

  #[test]
  fn test_get_edits_mbc() {
    let a = "const bar = \"👍🇺🇸😃\";\nconsole.log('hello deno')\n";
    let b = "const bar = \"👍🇺🇸😃\";\nconsole.log(\"hello deno\");\n";
    let actual = to_lsp_edits(get_range_utf16_edits(a, b, &None), &LineIndex::new(a));
    assert_eq!(
      actual,
      vec![
        TextEdit {
          range: lsp::Range {
            start: lsp::Position { line: 1, character: 12 },
            end: lsp::Position { line: 1, character: 13 }
          },
          new_text: "\"".to_string()
        },
        TextEdit {
          range: lsp::Range {
            start: lsp::Position { line: 1, character: 23 },
            end: lsp::Position { line: 1, character: 24 }
          },
          new_text: "\"".to_string()
        },
        TextEdit {
          range: lsp::Range {
            start: lsp::Position { line: 1, character: 25 },
            end: lsp::Position { line: 1, character: 25 }
          },
          new_text: ";".to_string()
        },
      ]
    )
  }

  #[test]
  fn should_only_keep_the_edits_within_the_range() {
    let get = |text: &str, formatted_text: &str, range: std::ops::Range<usize>| {
      get_range_utf16_edits(text, formatted_text, &Some(range))
        .into_iter()
        .map(|edit| (edit.start, edit.end, edit.new_text))
        .collect::<Vec<_>>()
    };
    // the indent of the next line isn't within the range's lines
    assert_eq!(get("a\nb", "a!\n  b", 0..2), vec![(1, 1, "!".to_string())]);
    // but an insertion at the end of a range without a line break is
    assert_eq!(get("a\nb", "a\nb!", 2..3), vec![(3, 3, "!".to_string())]);
    assert_eq!(get("a\nb", "!a\nb", 2..3), Vec::new());
  }

  #[test]
  fn should_apply_utf16_edits() {
    let edit = |start: u32, end: u32, new_text: &str| Utf16TextEdit {
      start,
      end,
      new_text: new_text.to_string(),
    };
    assert_eq!(apply_utf16_edits("abc", &[]), "abc");
    assert_eq!(apply_utf16_edits("abc", &[edit(0, 1, "x"), edit(2, 2, "y"), edit(3, 3, "z")]), "xbycz");
    // the offsets are UTF-16 code units
    assert_eq!(apply_utf16_edits("💾a💾b", &[edit(2, 3, "x"), edit(5, 6, "")]), "💾x💾");
  }

  #[test]
  fn should_map_utf16_range() {
    let edit = |start: u32, end: u32, new_text: &str| Utf16TextEdit {
      start,
      end,
      new_text: new_text.to_string(),
    };
    // an edit before the range moves it
    assert_eq!(map_utf16_range(4..6, &[edit(0, 1, "xyz")]), 6..8);
    assert_eq!(map_utf16_range(4..6, &[edit(4, 4, "xyz")]), 7..9);
    // an edit after it doesn't
    assert_eq!(map_utf16_range(4..6, &[edit(6, 7, "xyz")]), 4..6);
    // an edit within it changes its length
    assert_eq!(map_utf16_range(4..6, &[edit(5, 5, "xyz")]), 4..9);
    // an edit that overlaps it extends it to the edit's text
    assert_eq!(map_utf16_range(4..6, &[edit(3, 5, "xyz")]), 3..7);
    assert_eq!(map_utf16_range(4..6, &[edit(5, 8, "")]), 4..5);
    assert_eq!(map_utf16_range(4..6, &[edit(0, 10, "x")]), 0..1);
  }

  #[test]
  fn should_only_expand_the_range_for_plugins_that_do_not_format_one() {
    let range = RangeFormat::new("a\nbc\nd", Some(3..4), true);
    assert_eq!((range.plugin_range, range.edits_range), (Some(3..4), None));
    let range = RangeFormat::new("a\nbc\nd", Some(3..4), false);
    assert_eq!((range.plugin_range.clone(), range.edits_range.clone()), (Some(2..5), Some(2..5)));
    assert_eq!(range.cell_range(), Some(2..5));
    let range = RangeFormat::new("a\nbc\nd", None, false);
    assert_eq!((range.plugin_range, range.edits_range), (None, None));
  }
}
