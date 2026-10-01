use std::collections::HashMap;
use std::ops::Range;

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

  pub fn uris(&self) -> Vec<Uri> {
    self.docs.keys().cloned().collect()
  }

  pub fn get_content(&self, uri: &Uri) -> Option<(String, Option<LineIndex>)> {
    let Some(entry) = self.docs.get(uri) else {
      log_warn!(self.environment, "Missing document: {}", uri.as_str());
      return None;
    };
    Some((entry.text.clone(), entry.line_index.clone()))
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

  pub fn get_content_with_range(&mut self, uri: &Uri, lsp_range: lsp_types::Range) -> Option<(String, FormatRange, LineIndex)> {
    let Some(entry) = self.docs.get_mut(uri) else {
      log_warn!(self.environment, "Missing document: {}", uri.as_str());
      return None;
    };

    let line_index = entry.line_index.get_or_insert_with(|| LineIndex::new(&entry.text));
    let range = line_index.get_text_range(lsp_range).ok()?;
    Some((entry.text.clone(), Some(range.start().into()..range.end().into()), line_index.clone()))
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
        if !index_valid.covers(range.start.line) {
          line_index = LineIndex::new(&content);
        }
        index_valid = IndexValid::UpTo(range.start.line);
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
