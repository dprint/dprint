extern crate dprint_plugin_jsonc;
extern crate dprint_development;

//#[macro_use] extern crate debug_here;

use std::collections::HashMap;
use std::path::PathBuf;
// use std::time::Instant;

use dprint_core::configuration::*;
use dprint_development::*;
use dprint_plugin_jsonc::format_text;
use dprint_plugin_jsonc::configuration::{resolve_config};

#[test]
fn test_specs() {
    //debug_here!();
    let global_config = resolve_global_config(HashMap::new()).config;

    run_specs(
        &PathBuf::from("./tests/specs"),
        &ParseSpecOptions { default_file_name: "file.json" },
        &RunSpecsOptions { fix_failures: false, format_twice: true },
        move |_, file_text, spec_config| {
            let config_result = resolve_config(spec_config.clone(), &global_config);
            ensure_no_diagnostics(&config_result.diagnostics);

            format_text(&file_text, &config_result.config)
        }
    )
}
