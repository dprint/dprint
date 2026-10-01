/// Gets the file name to format a document of the language as when the document
/// isn't a file on the file system (ex. a notebook cell), which is what the plugin
/// that formats the document is selected by.
pub fn get_language_file_name(base_name: &str, language_id: &str) -> Option<String> {
  let language = language_id.to_ascii_lowercase();
  let ext = match known_language_extension(&language) {
    Some(ext) => ext,
    // fall back to the language id itself as the extension (ex. sql, toml, go)
    None if is_fallback_extension(&language) => &language,
    None => return None,
  };
  Some(format!("{}.{}", base_name, ext))
}

/// Gets the file extension for languages whose conventional extension differs
/// from the language id. This is kept in sync with the jupyter plugin so a cell
/// formats the same way in the editor as when the cli formats the notebook.
fn known_language_extension(language: &str) -> Option<&'static str> {
  Some(match language {
    "bash" | "sh" | "shell" | "shellscript" => "sh",
    "c#" | "csharp" => "cs",
    "c++" | "cpp" => "cpp",
    "clojure" => "clj",
    "coffeescript" => "coffee",
    "elixir" => "ex",
    "erlang" => "erl",
    "f#" | "fsharp" => "fs",
    "handlebars" => "hbs",
    "haskell" => "hs",
    "javascript" => "js",
    "javascriptreact" => "jsx",
    "julia" => "jl",
    "kotlin" => "kt",
    "latex" => "tex",
    "markdown" => "md",
    "nushell" => "nu",
    "ocaml" => "ml",
    "perl" => "pl",
    "powershell" => "ps1",
    "proto3" | "protobuf" => "proto",
    "python" | "python3" => "py",
    "restructuredtext" => "rst",
    "ruby" => "rb",
    "rust" => "rs",
    "terraform" => "tf",
    "typescript" => "ts",
    "typescriptreact" => "tsx",
    "yaml" => "yml",
    _ => return None,
  })
}

fn is_fallback_extension(language: &str) -> bool {
  !language.is_empty()
    && language.chars().all(|c| c.is_ascii_alphanumeric())
    // never format a cell as a notebook, which would recurse into the jupyter plugin
    && language != "ipynb"
}
