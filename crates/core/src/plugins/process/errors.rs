/// Formats an error and its source chain into a single string,
/// joining each level with `: ` (similar to formatting an
/// `anyhow` error with the alternate `{:#}` specifier).
pub fn error_to_string(err: &(dyn std::error::Error + 'static)) -> String {
  let mut result = err.to_string();
  let mut source = err.source();
  while let Some(err) = source {
    result.push_str(": ");
    result.push_str(&err.to_string());
    source = err.source();
  }
  result
}
