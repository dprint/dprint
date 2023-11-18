/// Checks for diagnostics and panics if it finds any.
pub fn ensure_no_diagnostics<T: std::fmt::Debug>(diagnostics: &[T]) {
  if let Some(diagnostic) = diagnostics.first() {
    panic!("Diagnostic error: {:?}", diagnostic);
  }
}
