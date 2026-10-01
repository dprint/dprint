use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use deno_tower_lsp::lsp_types::ClientCapabilities;
use deno_tower_lsp::lsp_types::DidChangeWatchedFilesRegistrationOptions;
use deno_tower_lsp::lsp_types::DocumentFilter;
use deno_tower_lsp::lsp_types::FileSystemWatcher;
use deno_tower_lsp::lsp_types::GlobPattern;
use deno_tower_lsp::lsp_types::InitializeParams;
use deno_tower_lsp::lsp_types::Registration;
use deno_tower_lsp::lsp_types::TextDocumentRegistrationOptions;
use deno_tower_lsp::lsp_types::Unregistration;
use serde::Deserialize;

const FORMATTING_METHOD: &str = "textDocument/formatting";
const RANGE_FORMATTING_METHOD: &str = "textDocument/rangeFormatting";

/// Options a client may provide when initializing the server.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct InitializationOptions {
  /// Whether to only provide formatting for the files in directories with a
  /// config file rather than for every file the client selects, which the
  /// server does by registering formatting for those directories as their
  /// files are opened.
  ///
  /// This is opt-in because it requires the client to match a file's path
  /// against the glob patterns the server registers. It allows a client to
  /// always start the server without dprint then being a formatter for the
  /// files of projects that don't use dprint.
  scoped_formatting: bool,
}

/// The directories whose files the server registered formatting for.
pub struct FormatScope {
  supports_range_formatting: bool,
  next_id: usize,
  ids_by_dir: HashMap<PathBuf, usize>,
}

impl FormatScope {
  /// Creates the format scope when the client opted into it and supports
  /// the server registering formatting.
  pub fn from_initialize_params(params: &InitializeParams) -> Option<Self> {
    let options = params
      .initialization_options
      .clone()
      .and_then(|options| serde_json::from_value::<InitializationOptions>(options).ok())
      .unwrap_or_default();
    let text_document = params.capabilities.text_document.as_ref()?;
    let supports = |dynamic_registration: Option<bool>| dynamic_registration == Some(true);
    if !options.scoped_formatting || !supports(text_document.formatting.as_ref().and_then(|c| c.dynamic_registration)) {
      return None;
    }
    Some(Self {
      supports_range_formatting: supports(text_document.range_formatting.as_ref().and_then(|c| c.dynamic_registration)),
      next_id: 0,
      ids_by_dir: Default::default(),
    })
  }

  /// Adds the directory, returning the registrations to send to the client
  /// when formatting wasn't already registered for it.
  pub fn add_dir(&mut self, dir: PathBuf) -> Vec<Registration> {
    if self.ids_by_dir.contains_key(&dir) {
      return Vec::new();
    }
    let id = self.next_id;
    self.next_id += 1;
    let options = serde_json::to_value(TextDocumentRegistrationOptions {
      document_selector: Some(vec![DocumentFilter {
        language: None,
        scheme: Some("file".to_string()),
        pattern: Some(dir_to_glob_pattern(&dir)),
      }]),
    })
    .unwrap();
    self.ids_by_dir.insert(dir, id);
    self
      .methods()
      .map(|method| Registration {
        id: registration_id(id, method),
        method: method.to_string(),
        register_options: Some(options.clone()),
      })
      .collect()
  }

  /// Sets the directories, returning what to unregister and register with the client.
  pub fn set_dirs(&mut self, dirs: HashSet<PathBuf>) -> (Vec<Unregistration>, Vec<Registration>) {
    let removed_dirs = self.ids_by_dir.keys().filter(|dir| !dirs.contains(*dir)).cloned().collect::<Vec<_>>();
    let mut unregistrations = Vec::new();
    for dir in removed_dirs {
      let id = self.ids_by_dir.remove(&dir).unwrap();
      unregistrations.extend(self.methods().map(|method| Unregistration {
        id: registration_id(id, method),
        method: method.to_string(),
      }));
    }
    // sorted for a deterministic order
    let mut dirs = dirs.into_iter().collect::<Vec<_>>();
    dirs.sort();
    let registrations = dirs.into_iter().flat_map(|dir| self.add_dir(dir)).collect();
    (unregistrations, registrations)
  }

  fn methods(&self) -> impl Iterator<Item = &'static str> {
    let range_formatting_method = self.supports_range_formatting.then_some(RANGE_FORMATTING_METHOD);
    std::iter::once(FORMATTING_METHOD).chain(range_formatting_method)
  }
}

/// Gets the registration that has the client tell the server when a config file
/// in the workspace is created, changed, or deleted, which is when the server
/// updates the directories it registered formatting for.
pub fn get_config_file_watcher_registration(capabilities: &ClientCapabilities) -> Option<Registration> {
  let did_change_watched_files = capabilities.workspace.as_ref()?.did_change_watched_files.as_ref()?;
  if did_change_watched_files.dynamic_registration != Some(true) {
    return None;
  }
  Some(Registration {
    id: "dprint-config-files".to_string(),
    method: "workspace/didChangeWatchedFiles".to_string(),
    register_options: Some(
      serde_json::to_value(DidChangeWatchedFilesRegistrationOptions {
        watchers: vec![FileSystemWatcher {
          glob_pattern: GlobPattern::String("**/{dprint,.dprint}.{json,jsonc}".to_string()),
          kind: None, // create, change, and delete
        }],
      })
      .unwrap(),
    ),
  })
}

fn registration_id(id: usize, method: &str) -> String {
  format!("dprint-format-scope-{}-{}", id, method)
}

/// Gets the glob pattern that matches the files in the directory and its
/// descendant directories when matched against a file's absolute path.
fn dir_to_glob_pattern(dir: &Path) -> String {
  let dir = dir.to_string_lossy().replace('\\', "/");
  let mut pattern = String::with_capacity(dir.len() + 5);
  let mut chars = dir.trim_end_matches('/').chars().peekable();
  // clients differ in the casing of a windows drive letter, so match both
  if let Some(drive_letter) = chars.peek().copied().filter(|c| c.is_ascii_alphabetic())
    && dir[1..].starts_with(":/")
  {
    chars.next();
    pattern.push('[');
    pattern.push(drive_letter.to_ascii_lowercase());
    pattern.push(drive_letter.to_ascii_uppercase());
    pattern.push(']');
  }
  for c in chars {
    // escape the characters that are special in a glob
    if matches!(c, '*' | '?' | '[' | '{' | '}') {
      pattern.push('[');
      pattern.push(c);
      pattern.push(']');
    } else {
      pattern.push(c);
    }
  }
  pattern.push_str("/**/*");
  pattern
}

#[cfg(test)]
mod tests {
  use super::*;

  fn create_scope(params: serde_json::Value) -> Option<FormatScope> {
    FormatScope::from_initialize_params(&serde_json::from_value(params).unwrap())
  }

  fn create_opted_in_scope(range_formatting: bool) -> FormatScope {
    create_scope(serde_json::json!({
      "initializationOptions": { "scopedFormatting": true },
      "capabilities": {
        "textDocument": {
          "formatting": { "dynamicRegistration": true },
          "rangeFormatting": { "dynamicRegistration": range_formatting },
        }
      }
    }))
    .unwrap()
  }

  #[test]
  fn test_from_initialize_params() {
    // not opted in
    assert!(
      create_scope(serde_json::json!({
        "capabilities": {
          "textDocument": { "formatting": { "dynamicRegistration": true } }
        }
      }))
      .is_none()
    );
    // can't register formatting
    assert!(
      create_scope(serde_json::json!({
        "initializationOptions": { "scopedFormatting": true },
        "capabilities": {}
      }))
      .is_none()
    );
  }

  #[test]
  fn test_add_dir() {
    let mut scope = create_opted_in_scope(true);
    let registrations = scope.add_dir(PathBuf::from("/dir"));
    assert_eq!(
      registrations.iter().map(|r| (r.id.as_str(), r.method.as_str())).collect::<Vec<_>>(),
      vec![
        ("dprint-format-scope-0-textDocument/formatting", "textDocument/formatting"),
        ("dprint-format-scope-0-textDocument/rangeFormatting", "textDocument/rangeFormatting"),
      ]
    );
    assert_eq!(
      registrations[0].register_options,
      Some(serde_json::json!({ "documentSelector": [{ "scheme": "file", "pattern": "/dir/**/*" }] }))
    );
    // already registered
    assert_eq!(scope.add_dir(PathBuf::from("/dir")), Vec::new());

    // without range formatting
    let mut scope = create_opted_in_scope(false);
    let registrations = scope.add_dir(PathBuf::from("/dir"));
    assert_eq!(
      registrations.iter().map(|r| r.method.as_str()).collect::<Vec<_>>(),
      vec!["textDocument/formatting"]
    );
  }

  #[test]
  fn test_set_dirs() {
    let mut scope = create_opted_in_scope(false);
    scope.add_dir(PathBuf::from("/a"));
    scope.add_dir(PathBuf::from("/b"));
    let (unregistrations, registrations) = scope.set_dirs(HashSet::from([PathBuf::from("/b"), PathBuf::from("/c")]));
    assert_eq!(
      unregistrations,
      vec![Unregistration {
        id: "dprint-format-scope-0-textDocument/formatting".to_string(),
        method: "textDocument/formatting".to_string(),
      }]
    );
    assert_eq!(
      registrations.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
      vec!["dprint-format-scope-2-textDocument/formatting"]
    );
    // no changes
    let (unregistrations, registrations) = scope.set_dirs(HashSet::from([PathBuf::from("/b"), PathBuf::from("/c")]));
    assert_eq!(unregistrations, Vec::new());
    assert_eq!(registrations, Vec::new());
  }

  #[test]
  fn test_dir_to_glob_pattern() {
    assert_eq!(dir_to_glob_pattern(Path::new("/home/user/project")), "/home/user/project/**/*");
    assert_eq!(dir_to_glob_pattern(Path::new("/home/user/project/")), "/home/user/project/**/*");
    assert_eq!(dir_to_glob_pattern(Path::new("C:\\Users\\user\\project")), "[cC]:/Users/user/project/**/*");
    assert_eq!(dir_to_glob_pattern(Path::new("/")), "/**/*");
    // special characters
    assert_eq!(dir_to_glob_pattern(Path::new("/dir/[id]/{a}*?")), "/dir/[[]id]/[{]a[}][*][?]/**/*");
  }

  #[test]
  fn test_get_config_file_watcher_registration() {
    fn get(capabilities: serde_json::Value) -> Option<Registration> {
      get_config_file_watcher_registration(&serde_json::from_value(capabilities).unwrap())
    }

    let registration = get(serde_json::json!({
      "workspace": { "didChangeWatchedFiles": { "dynamicRegistration": true } }
    }))
    .unwrap();
    assert_eq!(registration.method, "workspace/didChangeWatchedFiles");
    assert_eq!(
      registration.register_options,
      Some(serde_json::json!({ "watchers": [{ "globPattern": "**/{dprint,.dprint}.{json,jsonc}" }] }))
    );
    assert_eq!(get(serde_json::json!({ "workspace": { "didChangeWatchedFiles": {} } })), None);
    assert_eq!(get(serde_json::json!({})), None);
  }
}
