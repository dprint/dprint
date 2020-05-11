use dprint_core::configuration::ConfigurationDiagnostic;

/// Checks for diagnostics and panics if it finds any.
pub fn ensure_no_diagnostics(diagnostics: &Vec<ConfigurationDiagnostic>) {
    for diagnostic in diagnostics {
        panic!("Diagnostic error for '{}': {}", diagnostic.property_name, diagnostic.message);
    }
}