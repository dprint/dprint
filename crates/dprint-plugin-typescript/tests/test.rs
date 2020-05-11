extern crate dprint_plugin_typescript;
extern crate dprint_development;

//#[macro_use] extern crate debug_here;

use std::collections::HashMap;
use std::fs::{self};
use std::path::PathBuf;
use std::time::Instant;

use dprint_plugin_typescript::*;
use dprint_plugin_typescript::configuration::*;
use dprint_core::configuration::*;
use dprint_development::*;

#[allow(dead_code)]
fn test_performance() {
    // run this with `cargo test --release -- --nocapture`

    // This file was not written with an 80 line width in mind so overall
    // it's not too bad, but there are a few small issues to fix here and there.
    let config = ConfigurationBuilder::new()
        .line_width(80)
        .quote_style(QuoteStyle::PreferSingle)
        .build();
    let file_text = fs::read_to_string("tests/performance/checker.txt").expect("Expected to read.");
    let formatter = Formatter::new(config);

    //debug_here!();

    for i in 0..10 {
        let start = Instant::now();
        let result = formatter.format_text(&PathBuf::from("checker.ts"), &file_text).expect("Could not parse...");

        println!("{}ms", start.elapsed().as_millis());
        println!("---");

        if i == 0 {
            fs::write("tests/performance/checker_output.txt", result).expect("Expected to write to the file.");
        }
    }
}

#[test]
fn test_specs() {
    //debug_here!();
    let global_config = resolve_global_config(HashMap::new()).config;

    run_specs(
        &PathBuf::from("./tests/specs"),
        &ParseSpecOptions { default_file_name: "file.ts" },
        &RunSpecsOptions { fix_failures: false, format_twice: true },
        move |file_name, file_text, spec_config| {
            let config_result = resolve_config(spec_config.clone(), &global_config);
            ensure_no_diagnostics(&config_result.diagnostics);

            let formatter = Formatter::new(config_result.config);
            formatter.format_text(&file_name, &file_text)
        }
    )
}
