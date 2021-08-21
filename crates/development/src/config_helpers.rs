/// Checks for diagnostics and panics if it finds any.
pub fn ensure_no_diagnostics<T: std::fmt::Debug>(diagnostics: &[T]) {
  for diagnostic in diagnostics {
    panic!("Diagnostic error: {:?}", diagnostic);
  }
}
