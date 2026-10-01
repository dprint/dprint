use std::collections::HashMap;
use std::ops::Range;

use anyhow::Result;
use anyhow::bail;
use deno_tower_lsp::lsp_types;
use deno_tower_lsp::lsp_types::DidChangeTextDocumentParams;
use deno_tower_lsp::lsp_types::TextDocumentItem;
use deno_tower_lsp::lsp_types::Uri;
use dprint_core::plugins::FormatRange;

use crate::environment::Environment;

use super::text::LineIndex;

#[derive(Debug, PartialEq, Eq)]
enum IndexValid {
  All,
  UpTo(u32),
}

impl IndexValid {
  fn covers(&self, line: u32) -> bool {
    match *self {
      IndexValid::UpTo(to) => to > line,
      IndexValid::All => true,
    }
  }
}

pub struct Document {
  line_index: Option<LineIndex>,
  version: i32,
  pub language_id: String,
  pub text: String,
  /// The uri of the notebook when the document is a notebook cell.
  notebook_uri: Option<Uri>,
}

pub struct Documents<TEnvironment: Environment> {
  environment: TEnvironment,
  docs: HashMap<Uri, Document>,
}

impl<TEnvironment: Environment> Documents<TEnvironment> {
  pub fn new(environment: TEnvironment) -> Self {
    Self {
      environment,
      docs: Default::default(),
    }
  }

  pub fn open(&mut self, text_document_item: TextDocumentItem) {
    self.open_inner(text_document_item, None);
  }

  pub fn open_notebook_cell(&mut self, notebook_uri: &Uri, text_document_item: TextDocumentItem) {
    self.open_inner(text_document_item, Some(notebook_uri.clone()));
  }

  /// Gets the text of the document, which fails with the message to log when
  /// the document isn't open.
  pub fn get_content(&self, uri: &Uri) -> Result<(String, Option<LineIndex>)> {
    let Some(entry) = self.docs.get(uri) else {
      bail!("Missing document: {}", uri.as_str());
    };
    Ok((entry.text.clone(), entry.line_index.clone()))
  }

  pub fn get_language_id(&self, uri: &Uri) -> Option<String> {
    Some(self.docs.get(uri)?.language_id.clone())
  }

  /// Gets the uri of the notebook and the language of the cell when the
  /// document is a notebook cell.
  pub fn get_notebook_cell(&self, uri: &Uri) -> Option<(Uri, String)> {
    let entry = self.docs.get(uri)?;
    Some((entry.notebook_uri.clone()?, entry.language_id.clone()))
  }

  /// Gets the text of the document and the range within it, which fails with
  /// the message to log when the document isn't open or the range is invalid.
  pub fn get_content_with_range(&mut self, uri: &Uri, lsp_range: lsp_types::Range) -> Result<(String, FormatRange, LineIndex)> {
    let Some(entry) = self.docs.get_mut(uri) else {
      bail!("Missing document: {}", uri.as_str());
    };

    let line_index = entry.line_index.get_or_insert_with(|| LineIndex::new(&entry.text));
    let range = match line_index.get_text_range(lsp_range) {
      Ok(range) => range,
      Err(err) => bail!("Invalid range for '{}'. {:#}", uri.as_str(), err),
    };
    Ok((entry.text.clone(), Some(range.start().into()..range.end().into()), line_index.clone()))
  }

  pub fn changed(&mut self, params: DidChangeTextDocumentParams) {
    let Some(entry) = self.docs.get_mut(&params.text_document.uri) else {
      log_warn!(self.environment, "Missing document: {}", params.text_document.uri.as_str());
      return;
    };
    if entry.version > params.text_document.version {
      // the state has gone out of sync so it's no longer safe to format this document
      log_warn!(
        self.environment,
        "Changed version ({}) was less than existing version ({}) for '{}'. Forgetting document.",
        params.text_document.version,
        entry.version,
        params.text_document.uri.as_str(),
      );
      self.docs.remove(&params.text_document.uri);
      return;
    }
    let mut content = entry.text.to_string();
    let mut line_index = entry.line_index.take().unwrap_or_else(|| LineIndex::new(&content));
    let mut index_valid = IndexValid::All;
    for change in params.content_changes {
      if let Some(range) = change.range {
        // the index is only valid for the lines above the previous changes,
        // so check the end line because a range that starts above them may
        // end at or below them
        if !index_valid.covers(range.end.line) {
          line_index = LineIndex::new(&content);
        }
        // a start past the last line is the end of the text, which is
        // on the last line, so the index is not valid for that line
        index_valid = IndexValid::UpTo(range.start.line.min(line_index.last_line()));
        let range = match line_index.get_text_range(range) {
          Ok(range) => range,
          Err(err) => {
            log_warn!(
              self.environment,
              "Had error for '{}'. Forgetting document. {:#}",
              params.text_document.uri.as_str(),
              err
            );
            self.docs.remove(&params.text_document.uri);
            return;
          }
        };
        content.replace_range(Range::<usize>::from(range), &change.text);
      } else {
        content = change.text;
        index_valid = IndexValid::UpTo(0);
      }
    }
    if index_valid == IndexValid::All {
      entry.line_index = Some(line_index);
    }
    entry.text = content;
  }

  pub fn closed(&mut self, uri: &Uri) {
    self.docs.remove(uri);
  }

  pub fn closed_notebook(&mut self, notebook_uri: &Uri) {
    self.docs.retain(|_, doc| doc.notebook_uri.as_ref() != Some(notebook_uri));
  }

  fn open_inner(&mut self, text_document_item: TextDocumentItem, notebook_uri: Option<Uri>) {
    self.docs.insert(
      text_document_item.uri.clone(),
      Document {
        line_index: None,
        language_id: text_document_item.language_id,
        version: text_document_item.version,
        text: text_document_item.text,
        notebook_uri,
      },
    );
  }
}

#[cfg(test)]
mod test {
  use std::str::FromStr;

  use deno_tower_lsp::lsp_types::Position;
  use deno_tower_lsp::lsp_types::TextDocumentContentChangeEvent;
  use deno_tower_lsp::lsp_types::VersionedTextDocumentIdentifier;

  use crate::environment::TestEnvironment;

  use super::*;

  #[test]
  fn ranged_changes_in_one_notification_moving_down() {
    // the second change is below the first one, so the offsets of its lines
    // moved because of the first change
    assert_eq!(
      apply_changes("a\nb\nc\n", vec![ranged((0, 0), (0, 1), "aaaa\naa"), ranged((3, 0), (3, 1), "C")]),
      "aaaa\naa\nb\nC\n"
    );
    // on the same line as the first change
    assert_eq!(
      apply_changes("a\nb\nc\n", vec![ranged((1, 0), (1, 1), "bbbb"), ranged((1, 3), (1, 4), "X")]),
      "a\nbbbX\nc\n"
    );
  }

  #[test]
  fn ranged_changes_in_one_notification_moving_up() {
    // entirely above the first change
    assert_eq!(
      apply_changes("a\nb\nc\n", vec![ranged((2, 0), (2, 1), "cccc"), ranged((0, 0), (0, 1), "A")]),
      "A\nb\ncccc\n"
    );
    // starts above the first change and ends on its line
    assert_eq!(
      apply_changes("a\nb\nc\n", vec![ranged((1, 0), (1, 1), "bbbb"), ranged((0, 0), (1, 4), "X")]),
      "X\nc\n"
    );
    // starts above the first change and ends below it
    assert_eq!(
      apply_changes("a\nb\nc\n", vec![ranged((1, 0), (1, 1), "bbbb"), ranged((0, 0), (2, 1), "X")]),
      "X\n"
    );
    assert_eq!(
      apply_changes(
        "a\nb\nc\nd\n",
        vec![ranged((2, 0), (2, 1), "c\nc\nc"), ranged((1, 0), (1, 1), "bbbb"), ranged((0, 1), (5, 1), "X")]
      ),
      "aX\n"
    );
  }

  #[test]
  fn ranged_change_after_change_starting_past_last_line() {
    // the first change is at the end of the text, which is on the last line,
    // so the second change can't use the line index for that line
    assert_eq!(apply_changes("a\nb", vec![ranged((5, 0), (5, 0), "xyz"), ranged((1, 0), (1, 4), "")]), "a\n");
    assert_eq!(apply_changes("a\nb", vec![ranged((2, 0), (2, 0), "xyz"), ranged((1, 0), (1, 4), "")]), "a\n");
    // the last line is the empty one after the final newline
    assert_eq!(
      apply_changes("a\nb\n", vec![ranged((3, 0), (3, 0), "xyz"), ranged((2, 0), (2, 3), "Q")]),
      "a\nb\nQ"
    );
    // starts above the last line and ends on it
    assert_eq!(apply_changes("a\nb\nc", vec![ranged((9, 0), (9, 0), "xyz"), ranged((1, 0), (2, 4), "")]), "a\n");
    // the lines above the last one are still found after such a change
    assert_eq!(
      apply_changes(
        "a\nb\nc",
        vec![ranged((9, 0), (9, 0), "\nd"), ranged((1, 0), (1, 1), "B"), ranged((0, 0), (0, 1), "A")]
      ),
      "A\nB\nc\nd"
    );
    // empty text
    assert_eq!(apply_changes("", vec![ranged((4, 0), (4, 0), "xyz"), ranged((0, 0), (0, 3), "Q")]), "Q");
  }

  #[test]
  fn ranged_change_after_surrogate_pair() {
    // the columns are in utf-16 code units and the emoji is two of them
    assert_eq!(apply_changes("a😀b\nc\n", vec![ranged((0, 3), (0, 4), "X")]), "a😀X\nc\n");
    assert_eq!(
      apply_changes("ab\nc\n", vec![ranged((0, 1), (0, 1), "😀"), ranged((0, 3), (0, 4), "X")]),
      "a😀X\nc\n"
    );
    // a change above one that added a surrogate pair
    assert_eq!(
      apply_changes("a\nb\nc\n", vec![ranged((1, 0), (1, 0), "😀"), ranged((0, 0), (1, 3), "X")]),
      "X\nc\n"
    );
  }

  #[test]
  fn full_text_change_mixed_with_ranged_changes() {
    assert_eq!(
      apply_changes(
        "a\nb\nc\n",
        vec![
          ranged((2, 0), (2, 1), "cccc"),
          full("one\ntwo\nthree\nfour\n"),
          ranged((3, 0), (3, 4), "4"),
          ranged((0, 0), (1, 3), "1\n2"),
        ]
      ),
      "1\n2\nthree\n4\n"
    );
    assert_eq!(apply_changes("a\nb\nc\n", vec![ranged((0, 0), (0, 1), "A"), full("one\ntwo\n")]), "one\ntwo\n");
  }

  #[test]
  fn ranged_changes_with_cached_line_index() {
    let (mut documents, uri) = open_document("a\nb\nc\n");
    // this caches the line index in the document
    let range = lsp_types::Range::new(Position::new(1, 0), Position::new(1, 1));
    let (_, format_range, _) = documents.get_content_with_range(&uri, range).unwrap();
    assert_eq!(format_range, Some(2..3));
    change(&mut documents, &uri, vec![ranged((1, 0), (1, 1), "bbbb"), ranged((0, 0), (2, 1), "X")]);
    assert_eq!(documents.get_content(&uri).unwrap().0, "X\n");
    // the line index is for the new text
    let range = lsp_types::Range::new(Position::new(0, 1), Position::new(1, 0));
    let (text, format_range, _) = documents.get_content_with_range(&uri, range).unwrap();
    assert_eq!(text, "X\n");
    assert_eq!(format_range, Some(1..2));
  }

  fn apply_changes(text: &str, changes: Vec<TextDocumentContentChangeEvent>) -> String {
    let (mut documents, uri) = open_document(text);
    change(&mut documents, &uri, changes);
    documents.get_content(&uri).unwrap().0
  }

  fn open_document(text: &str) -> (Documents<TestEnvironment>, Uri) {
    let uri = Uri::from_str("file:///file.txt").unwrap();
    let mut documents = Documents::new(TestEnvironment::new());
    documents.open(TextDocumentItem {
      uri: uri.clone(),
      language_id: "txt".to_string(),
      version: 0,
      text: text.to_string(),
    });
    (documents, uri)
  }

  fn change(documents: &mut Documents<TestEnvironment>, uri: &Uri, content_changes: Vec<TextDocumentContentChangeEvent>) {
    documents.changed(DidChangeTextDocumentParams {
      text_document: VersionedTextDocumentIdentifier { uri: uri.clone(), version: 1 },
      content_changes,
    });
  }

  fn ranged(start: (u32, u32), end: (u32, u32), text: &str) -> TextDocumentContentChangeEvent {
    TextDocumentContentChangeEvent {
      range: Some(lsp_types::Range::new(Position::new(start.0, start.1), Position::new(end.0, end.1))),
      range_length: None,
      text: text.to_string(),
    }
  }

  fn full(text: &str) -> TextDocumentContentChangeEvent {
    TextDocumentContentChangeEvent {
      range: None,
      range_length: None,
      text: text.to_string(),
    }
  }
}
