use super::*;
use super::configuration::Configuration;

#[deprecated(since = "0.11.0", note = "Please use the Formatter struct instead.")]
pub fn format_text(file_path: &str, file_text: &str, config: &Configuration) -> Result<Option<String>, String> {
    let formatter = formatter::Formatter::new(config);
    formatter.format_text(file_path, file_text)
}
