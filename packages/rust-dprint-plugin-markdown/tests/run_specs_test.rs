extern crate dprint_plugin_markdown;
extern crate dprint_development;

use std::collections::HashMap;
use std::path::PathBuf;

use dprint_development::*;
use dprint_core::configuration::*;
use dprint_plugin_markdown::configuration::*;
use dprint_plugin_markdown::*;

#[test]
fn test_specs() {
    //debug_here!();
    let global_config = resolve_global_config(HashMap::new()).config;

    run_specs(
        &PathBuf::from("./tests/specs"),
        &ParseSpecOptions { default_file_name: "file.md" },
        &RunSpecsOptions { fix_failures: false, format_twice: true },
        move |_, file_text, spec_config| {
            let config_result = resolve_config(spec_config.clone(), &global_config);
            ensure_no_diagnostics(&config_result.diagnostics);

            format_text(&file_text, &config_result.config)
        }
    );
}
