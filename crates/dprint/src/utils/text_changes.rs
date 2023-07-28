// Lifted from code I wrote here:
//   https://github.com/denoland/deno_ast/blob/0074ac42a1a57e7805c0c4cc03b95a5717b47f3a/src/text_changes.rs
// Copyright 2018-2023 the Deno authors. All rights reserved. MIT license.

use anyhow::bail;
use anyhow::Result;
use std::cmp::Ordering;
use std::ops::Range;

#[derive(Clone, Debug)]
pub struct TextChange {
  /// Range start to end byte index.
  pub range: Range<usize>,
  /// New text to insert or replace at the provided range.
  pub new_text: String,
}

impl TextChange {
  #[cfg(test)]
  pub fn new(start: usize, end: usize, new_text: String) -> Self {
    Self { range: start..end, new_text }
  }
}

/// Applies the text changes to the given source text.
pub fn apply_text_changes(source: &str, mut changes: Vec<TextChange>) -> Result<String> {
  changes.sort_by(|a, b| match a.range.start.cmp(&b.range.start) {
    Ordering::Equal => a.range.end.cmp(&b.range.end),
    ordering => ordering,
  });

  let mut last_index = 0;
  let mut final_text = String::new();

  for (i, change) in changes.iter().enumerate() {
    if change.range.start > change.range.end {
      bail!(
        "Text change had start index {} greater than end index {}.\n\n{:?}",
        change.range.start,
        change.range.end,
        &changes[0..i + 1],
      )
    }
    if change.range.start < last_index {
      bail!(
        "Text changes were overlapping. Past index was {}, but new change had index {}.\n\n{:?}",
        last_index,
        change.range.start,
        &changes[0..i + 1]
      );
    } else if change.range.start > last_index && last_index < source.len() {
      final_text.push_str(&source[last_index..std::cmp::min(source.len(), change.range.start)]);
    }
    final_text.push_str(&change.new_text);
    last_index = change.range.end;
  }

  if last_index < source.len() {
    final_text.push_str(&source[last_index..]);
  }

  Ok(final_text)
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn applies_text_changes() {
    // replacing text
    assert_eq!(
      apply_text_changes(
        "0123456789",
        vec![
          TextChange::new(9, 10, "z".to_string()),
          TextChange::new(4, 6, "y".to_string()),
          TextChange::new(1, 2, "x".to_string()),
        ]
      )
      .unwrap(),
      "0x23y678z".to_string(),
    );

    // replacing beside
    assert_eq!(
      apply_text_changes(
        "0123456789",
        vec![
          TextChange::new(0, 5, "a".to_string()),
          TextChange::new(5, 7, "b".to_string()),
          TextChange::new(7, 10, "c".to_string()),
        ]
      )
      .unwrap(),
      "abc".to_string(),
    );

    // full replace
    assert_eq!(
      apply_text_changes("0123456789", vec![TextChange::new(0, 10, "x".to_string()),]).unwrap(),
      "x".to_string(),
    );

    // 1 over
    assert_eq!(
      apply_text_changes("0123456789", vec![TextChange::new(0, 11, "x".to_string()),]).unwrap(),
      "x".to_string(),
    );

    // insert
    assert_eq!(
      apply_text_changes("0123456789", vec![TextChange::new(5, 5, "x".to_string()),]).unwrap(),
      "01234x56789".to_string(),
    );

    // prepend
    assert_eq!(
      apply_text_changes("0123456789", vec![TextChange::new(0, 0, "x".to_string()),]).unwrap(),
      "x0123456789".to_string(),
    );

    // append
    assert_eq!(
      apply_text_changes("0123456789", vec![TextChange::new(10, 10, "x".to_string()),]).unwrap(),
      "0123456789x".to_string(),
    );

    // append over
    assert_eq!(
      apply_text_changes("0123456789", vec![TextChange::new(11, 11, "x".to_string()),]).unwrap(),
      "0123456789x".to_string(),
    );

    // multiple at start
    assert_eq!(
      apply_text_changes(
        "0123456789",
        vec![
          TextChange::new(0, 7, "a".to_string()),
          TextChange::new(0, 0, "b".to_string()),
          TextChange::new(0, 0, "c".to_string()),
          TextChange::new(7, 10, "d".to_string()),
        ]
      )
      .unwrap(),
      "bcad".to_string(),
    );
  }

  #[test]
  fn errors_text_change_within() {
    assert_eq!(
      apply_text_changes(
        "0123456789",
        vec![TextChange::new(3, 10, "x".to_string()), TextChange::new(5, 7, "x".to_string())],
      )
      .err()
      .unwrap()
      .to_string(),
      "Text changes were overlapping. Past index was 10, but new change had index 5.\n\n[TextChange { range: 3..10, new_text: \"x\" }, TextChange { range: 5..7, new_text: \"x\" }]"
    );
  }

  #[test]
  fn errors_text_change_overlap() {
    assert_eq!(
      apply_text_changes(
        "0123456789",
        vec![TextChange::new(2, 4, "x".to_string()), TextChange::new(3, 5, "x".to_string())],
      )
      .err()
      .unwrap()
      .to_string(),
      "Text changes were overlapping. Past index was 4, but new change had index 3.\n\n[TextChange { range: 2..4, new_text: \"x\" }, TextChange { range: 3..5, new_text: \"x\" }]"
    );
  }

  #[test]
  fn errors_start_greater_end() {
    assert_eq!(
      apply_text_changes("0123456789", vec![TextChange::new(2, 1, "x".to_string())])
        .err()
        .unwrap()
        .to_string(),
      "Text change had start index 2 greater than end index 1.\n\n[TextChange { range: 2..1, new_text: \"x\" }]"
    );
  }
}
