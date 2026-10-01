use std::path::Path;
use std::path::PathBuf;

use deno_tower_lsp::lsp_types::ClientCapabilities;
use deno_tower_lsp::lsp_types::DocumentFilter;
use deno_tower_lsp::lsp_types::Registration;
use deno_tower_lsp::lsp_types::TextDocumentChangeRegistrationOptions;
use deno_tower_lsp::lsp_types::TextDocumentRegistrationOptions;
use deno_tower_lsp::lsp_types::TextDocumentSyncKind;
use deno_tower_lsp::lsp_types::Uri;

use super::language::get_language_file_name;

/// The scheme of the uris vscode gives new documents that haven't been saved.
const UNTITLED_SCHEME: &str = "untitled";

pub fn is_untitled_uri(uri: &Uri) -> bool {
  uri.scheme().as_str().eq_ignore_ascii_case(UNTITLED_SCHEME)
}

/// Gets the registrations that have the client sync and format untitled documents.
/// The server registers these instead of leaving it up to the client's document
/// selector so that a client only sends untitled documents to a version of dprint
/// that formats them.
pub fn get_untitled_registrations(capabilities: &ClientCapabilities) -> Vec<Registration> {
  let Some(text_document) = &capabilities.text_document else {
    return Vec::new();
  };
  let supports = |dynamic_registration: Option<bool>| dynamic_registration == Some(true);
  // syncing and formatting the documents are both necessary
  if !supports(text_document.synchronization.as_ref().and_then(|c| c.dynamic_registration))
    || !supports(text_document.formatting.as_ref().and_then(|c| c.dynamic_registration))
  {
    return Vec::new();
  }
  let document_selector = Some(vec![DocumentFilter {
    language: None,
    scheme: Some(UNTITLED_SCHEME.to_string()),
    pattern: None,
  }]);
  let options = serde_json::to_value(TextDocumentRegistrationOptions {
    document_selector: document_selector.clone(),
  })
  .unwrap();
  let change_options = serde_json::to_value(TextDocumentChangeRegistrationOptions {
    document_selector,
    // matches the server's text document sync capability
    sync_kind: TextDocumentSyncKind::FULL,
  })
  .unwrap();
  let mut registrations = vec![
    ("textDocument/didOpen", options.clone()),
    ("textDocument/didChange", change_options),
    ("textDocument/didClose", options.clone()),
    ("textDocument/formatting", options.clone()),
  ];
  if supports(text_document.range_formatting.as_ref().and_then(|c| c.dynamic_registration)) {
    registrations.push(("textDocument/rangeFormatting", options));
  }
  registrations
    .into_iter()
    .map(|(method, options)| Registration {
      id: format!("dprint-untitled-{}", method),
      method: method.to_string(),
      register_options: Some(options),
    })
    .collect()
}

/// Gets the file path to format an untitled document as. An untitled document
/// isn't on the file system, so it's formatted as a file in the provided
/// directory that's named based on its language.
pub fn get_untitled_file_path(dir_path: &Path, language_id: &str) -> Option<PathBuf> {
  Some(dir_path.join(get_language_file_name("Untitled", language_id)?))
}

#[cfg(test)]
mod tests {
  use std::str::FromStr;

  use super::*;

  #[test]
  fn test_is_untitled_uri() {
    assert!(is_untitled_uri(&Uri::from_str("untitled:Untitled-1").unwrap()));
    assert!(!is_untitled_uri(&Uri::from_str("file:///Untitled-1").unwrap()));
  }

  #[test]
  fn test_get_untitled_registrations() {
    fn get(capabilities: serde_json::Value) -> Vec<String> {
      get_untitled_registrations(&serde_json::from_value(capabilities).unwrap())
        .into_iter()
        .map(|r| r.method)
        .collect()
    }

    assert_eq!(
      get(serde_json::json!({
        "textDocument": {
          "synchronization": { "dynamicRegistration": true },
          "formatting": { "dynamicRegistration": true },
          "rangeFormatting": { "dynamicRegistration": true },
        }
      })),
      vec![
        "textDocument/didOpen",
        "textDocument/didChange",
        "textDocument/didClose",
        "textDocument/formatting",
        "textDocument/rangeFormatting",
      ]
    );
    assert_eq!(
      get(serde_json::json!({
        "textDocument": {
          "synchronization": { "dynamicRegistration": true },
          "formatting": { "dynamicRegistration": true },
        }
      })),
      vec![
        "textDocument/didOpen",
        "textDocument/didChange",
        "textDocument/didClose",
        "textDocument/formatting",
      ]
    );
    // can't sync the documents
    assert_eq!(
      get(serde_json::json!({
        "textDocument": {
          "formatting": { "dynamicRegistration": true },
          "rangeFormatting": { "dynamicRegistration": true },
        }
      })),
      Vec::<String>::new()
    );
    assert_eq!(get(serde_json::json!({})), Vec::<String>::new());
  }

  #[test]
  fn test_get_untitled_registration_options() {
    let registrations = get_untitled_registrations(
      &serde_json::from_value(serde_json::json!({
        "textDocument": {
          "synchronization": { "dynamicRegistration": true },
          "formatting": { "dynamicRegistration": true },
        }
      }))
      .unwrap(),
    );
    assert_eq!(
      registrations[0].register_options,
      Some(serde_json::json!({ "documentSelector": [{ "scheme": "untitled" }] }))
    );
    assert_eq!(
      registrations[1].register_options,
      Some(serde_json::json!({ "documentSelector": [{ "scheme": "untitled" }], "syncKind": 1 }))
    );
  }

  #[test]
  fn test_get_untitled_file_path() {
    assert_eq!(
      get_untitled_file_path(Path::new("/dir"), "typescript"),
      Some(PathBuf::from("/dir").join("Untitled.ts"))
    );
    assert_eq!(get_untitled_file_path(Path::new("/dir"), "objective-c"), None);
  }
}
