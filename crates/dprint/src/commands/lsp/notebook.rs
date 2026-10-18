use std::path::Path;
use std::path::PathBuf;

use deno_tower_lsp::lsp_types::ClientCapabilities;
use deno_tower_lsp::lsp_types::Notebook;
use deno_tower_lsp::lsp_types::NotebookCellTextDocumentFilter;
use deno_tower_lsp::lsp_types::NotebookDocumentFilter;
use deno_tower_lsp::lsp_types::NotebookDocumentSyncOptions;
use deno_tower_lsp::lsp_types::NotebookSelector;
use deno_tower_lsp::lsp_types::Registration;
use dprint_core::plugins::FormatRange;

use super::language::get_language_file_name;

/// Gets the capability that has the client sync the cells of notebooks on the
/// file system, which are the only notebooks the cli formats.
pub fn get_notebook_document_sync_options() -> NotebookDocumentSyncOptions {
  NotebookDocumentSyncOptions {
    notebook_selector: vec![NotebookSelector::ByNotebook {
      notebook: file_system_notebook(),
      cells: None, // all the cells
    }],
    save: None,
  }
}

/// Gets the registrations that have the client format the documents of notebook
/// cells. These need to be registered because the server's formatting capabilities
/// can't specify the documents they're for, so they only apply to the documents
/// the client selects, which a client isn't going to have include notebook cells.
pub fn get_notebook_cell_format_registrations(capabilities: &ClientCapabilities) -> Vec<Registration> {
  // the client won't have synced the cells when it doesn't support notebooks
  if capabilities.notebook_document.is_none() {
    return Vec::new();
  }
  let Some(text_document) = &capabilities.text_document else {
    return Vec::new();
  };
  let options = serde_json::json!({
    "documentSelector": [NotebookCellTextDocumentFilter {
      notebook: file_system_notebook(),
      language: None,
    }],
  });
  [
    ("textDocument/formatting", &text_document.formatting),
    ("textDocument/rangeFormatting", &text_document.range_formatting),
  ]
  .into_iter()
  .filter(|(_, capability)| capability.as_ref().and_then(|c| c.dynamic_registration) == Some(true))
  .map(|(method, _)| Registration {
    id: format!("dprint-notebook-cell-{}", method),
    method: method.to_string(),
    register_options: Some(options.clone()),
  })
  .collect()
}

/// Gets the file path to format a notebook cell as, which is what the plugin
/// that formats the cell is selected by. It's a file in the notebook's directory
/// named based on the cell's language, which is how the jupyter plugin formats
/// a notebook's code cells.
pub fn get_notebook_cell_file_path(notebook_path: &Path, language_id: &str) -> Option<PathBuf> {
  Some(notebook_path.parent()?.join(get_language_file_name("code_block", language_id)?))
}

/// Trims the trailing whitespace of a notebook cell's formatted text like the jupyter
/// plugin does because many plugins add a final newline, which doesn't look nice in a
/// cell. When formatting a range that ends before the end of the cell, the cell's
/// original trailing whitespace is kept so the text after the range stays the same.
pub fn trim_formatted_cell_text(original_text: &str, mut formatted_text: String, range: &FormatRange) -> String {
  formatted_text.truncate(formatted_text.trim_end().len());
  if let Some(range) = range
    && range.end < original_text.len()
  {
    formatted_text.push_str(&original_text[original_text.trim_end().len()..]);
  }
  formatted_text
}

fn file_system_notebook() -> Notebook {
  Notebook::NotebookDocumentFilter(NotebookDocumentFilter::ByScheme {
    notebook_type: None,
    scheme: "file".to_string(),
    pattern: None,
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_get_notebook_document_sync_options() {
    assert_eq!(
      serde_json::to_value(get_notebook_document_sync_options()).unwrap(),
      serde_json::json!({ "notebookSelector": [{ "notebook": { "scheme": "file" } }] })
    );
  }

  #[test]
  fn test_get_notebook_cell_format_registrations() {
    fn get(capabilities: serde_json::Value) -> Vec<String> {
      let registrations = get_notebook_cell_format_registrations(&serde_json::from_value(capabilities).unwrap());
      for registration in &registrations {
        assert_eq!(
          registration.register_options,
          Some(serde_json::json!({ "documentSelector": [{ "notebook": { "scheme": "file" } }] }))
        );
      }
      registrations.into_iter().map(|r| r.method).collect()
    }

    assert_eq!(
      get(serde_json::json!({
        "notebookDocument": { "synchronization": {} },
        "textDocument": {
          "formatting": { "dynamicRegistration": true },
          "rangeFormatting": { "dynamicRegistration": true },
        }
      })),
      vec!["textDocument/formatting", "textDocument/rangeFormatting"]
    );
    assert_eq!(
      get(serde_json::json!({
        "notebookDocument": { "synchronization": {} },
        "textDocument": {
          "formatting": { "dynamicRegistration": true },
          "rangeFormatting": { "dynamicRegistration": false },
        }
      })),
      vec!["textDocument/formatting"]
    );
    // doesn't support notebooks
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
  fn test_get_notebook_cell_file_path() {
    fn get(language_id: &str) -> Option<PathBuf> {
      get_notebook_cell_file_path(Path::new("/dir/notebook.ipynb"), language_id)
    }

    // known languages whose extension differs from the language id
    assert_eq!(get("python"), Some(PathBuf::from("/dir/code_block.py")));
    assert_eq!(get("TypeScript"), Some(PathBuf::from("/dir/code_block.ts")));
    assert_eq!(get("markdown"), Some(PathBuf::from("/dir/code_block.md")));
    // other languages use the language id as the extension
    assert_eq!(get("sql"), Some(PathBuf::from("/dir/code_block.sql")));
    // languages that can't be used as an extension
    assert_eq!(get(""), None);
    assert_eq!(get("objective-c"), None);
    assert_eq!(get("ipynb"), None);
  }

  #[test]
  fn test_trim_formatted_cell_text() {
    fn trim(original_text: &str, formatted_text: &str, range: FormatRange) -> String {
      trim_formatted_cell_text(original_text, formatted_text.to_string(), &range)
    }

    assert_eq!(trim("a  ", "a\n", None), "a");
    // range to the end of the cell
    assert_eq!(trim("a;b \n", "a;\nb;\n", Some(2..5)), "a;\nb;");
    // range ending before the end of the cell
    assert_eq!(trim("a;b \n", "a;\nb\n", Some(0..2)), "a;\nb \n");
  }
}
