extern crate dprint_plugin_markdown;
extern crate dprint_development;

use std::collections::HashMap;
use std::path::Path;

use dprint_development::*;
use dprint_core::configuration::*;
use dprint_plugin_markdown::configuration::*;
use dprint_plugin_markdown::*;

#[test]
fn test_specs() {
    //debug_here!();
    let global_config = resolve_global_config(&HashMap::new()).config;

    run_specs(&Path::new("./tests/specs"), &ParseSpecOptions { default_file_name: "file.md" }, move |_, file_text, spec_config| {
        let config_result = resolve_config(&spec_config, &global_config);
        ensure_no_diagnostics(&config_result.diagnostics);

        format_text(&file_text, &config_result.config)
    });
}
