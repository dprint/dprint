use super::configuration::Configuration;

/// Formats a file.
///
/// Returns the file text when the file was formatted, `None` when the file had an ignore comment, and
/// an error when it failed to parse.
pub fn format_text(file_path: &str, file_text: &str, config: &Configuration) -> Result<Option<String>, String> {
    Err(String::from("not implemented"))
}