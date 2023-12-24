use std::collections::HashMap;
use std::ops::Range;

use dprint_core::plugins::FormatRange;
use tower_lsp::lsp_types;
use tower_lsp::lsp_types::DidChangeTextDocumentParams;
use tower_lsp::lsp_types::DidCloseTextDocumentParams;
use tower_lsp::lsp_types::TextDocumentItem;
use url::Url;

use crate::environment::Environment;

use super::client::ClientWrapper;
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
}

pub struct Documents<TEnvironment: Environment> {
  client: ClientWrapper,
  environment: TEnvironment,
  docs: HashMap<Url, Document>,
}

impl<TEnvironment: Environment> Documents<TEnvironment> {
  pub fn new(client: ClientWrapper, environment: TEnvironment) -> Self {
    Self {
      client,
      environment,
      docs: Default::default(),
    }
  }

  pub fn open(&mut self, text_document_item: TextDocumentItem) {
    self.docs.insert(
      text_document_item.uri.clone(),
      Document {
        line_index: None,
        language_id: text_document_item.language_id,
        version: text_document_item.version,
        text: text_document_item.text,
      },
    );
  }

  pub fn get_content(&self, uri: &Url) -> Option<(String, Option<LineIndex>)> {
    let Some(entry) = self.docs.get(uri) else {
      log_warn!(self.environment, "Missing document: {}", uri);
      return None;
    };
    Some((entry.text.clone(), entry.line_index.clone()))
  }

  pub fn get_content_with_range(&mut self, uri: &Url, lsp_range: lsp_types::Range) -> Option<(String, FormatRange, LineIndex)> {
    let Some(entry) = self.docs.get_mut(uri) else {
      log_warn!(self.environment, "Missing document: {}", uri);
      return None;
    };

    let line_index = entry.line_index.get_or_insert_with(|| LineIndex::new(&entry.text));
    let range = line_index.get_text_range(lsp_range).ok()?;
    Some((entry.text.clone(), Some(range.start().into()..range.end().into()), line_index.clone()))
  }

  pub fn changed(&mut self, params: DidChangeTextDocumentParams) {
    let Some(entry) = self.docs.get_mut(&params.text_document.uri) else {
      log_warn!(self.environment, "Missing document: {}", params.text_document.uri);
      return;
    };
    if entry.version > params.text_document.version {
      // the state has gone out of sync so it's no longer safe to format this document
      log_warn!(
        self.environment,
        "Changed version ({}) was less than existing version ({}) for '{}'. Forgetting document.",
        params.text_document.version,
        entry.version,
        params.text_document.uri,
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
            log_warn!(self.environment, "Had error for '{}'. Forgetting document. {:#}", params.text_document.uri, err);
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

  pub fn closed(&mut self, params: DidCloseTextDocumentParams) {
    self.docs.remove(&params.text_document.uri);
  }
}
