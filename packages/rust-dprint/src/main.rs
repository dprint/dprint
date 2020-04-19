extern crate dprint_core;
extern crate dprint_plugin_typescript as dprint_ts;

use clap::{App, Arg, Values};
use rayon::prelude::*;
use dprint_ts::configuration::{Configuration, ConfigurationBuilder};
use serde_json;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

// todo: mock file system then add tests

fn main() {
    let cli_parser = create_cli_parser();
    let matches = cli_parser.get_matches();

    let config = resolve_config(matches.value_of("config"));
    let check = matches.is_present("check");
    let files = resolve_file_paths(matches.values_of("files"));

    if check {
        match check_files(config, files) {
            Ok(_) => {},
            Err(e) => {
                eprintln!("{}", e);
                std::process::exit(1);
            },
        }
    } else {
        format_files(config, files);
    }
}

fn check_files(config: Configuration, paths: Vec<PathBuf>) -> Result<(), String> {
    let not_formatted_files_count = AtomicUsize::new(0);
    let formatter = dprint_ts::Formatter::new(config);

    paths.par_iter().for_each(|file_path| {
        let file_path_str = file_path.to_string_lossy();
        let file_contents = fs::read_to_string(&file_path);
        match file_contents {
            Ok(file_contents) => {
                match formatter.format_text(&file_path_str, &file_contents) {
                    Ok(None) => {
                        // nothing to format, pass
                    }
                    Ok(Some(_)) => {
                        not_formatted_files_count.fetch_add(1, Ordering::SeqCst);
                    }
                    Err(e) => {
                        output_error(&file_path_str, "Error checking", &e);
                    },
                }
            },
            Err(e) => {
                output_error(&file_path_str, "Error reading file", &e);
            },
        }
    });

    let not_formatted_files_count = not_formatted_files_count.load(Ordering::SeqCst);
    if not_formatted_files_count == 0 {
        Ok(())
    } else {
        let f = if not_formatted_files_count == 1 { "file" } else { "files" };
        Err(format!("Found {} not formatted {}", not_formatted_files_count, f))
    }
}

fn format_files(config: Configuration, paths: Vec<PathBuf>) {
    let formatted_files_count = AtomicUsize::new(0);
    let files_count = paths.len();
    let formatter = dprint_ts::Formatter::new(config);

    paths.par_iter().for_each(|file_path| {
        let file_path_str = file_path.to_string_lossy();
        let file_contents = fs::read_to_string(&file_path);

        match file_contents {
            Ok(file_contents) => {
                match formatter.format_text(&file_path_str, &file_contents) {
                    Ok(None) => {
                        // nothing to format, pass
                    }
                    Ok(Some(formatted_text)) => {
                        println!("{}", file_path_str);
                        match fs::write(&file_path, formatted_text) {
                            Ok(_) => {
                                formatted_files_count.fetch_add(1, Ordering::SeqCst);
                            },
                            Err(e) => output_error(&file_path_str, "Error writing file", &e),
                        };
                    }
                    Err(e) => output_error(&file_path_str, "Error formatting", &e),
                }
            },
            Err(e) => output_error(&file_path_str, "Error reading file", &e),
        }
    });

    let formatted_files_count = formatted_files_count.load(Ordering::SeqCst);
    let suffix = if files_count == 1 { "file" } else { "files" };
    println!("Formatted {} {}", formatted_files_count, suffix);
}

fn output_error(file_path_str: &str, text: &str, error: &impl std::fmt::Display) {
    eprintln!("{}: {}", text, &file_path_str);
    eprintln!("    {}", error);
}

fn resolve_config(config_path: Option<&str>) -> Configuration {
    if let Some(config_path) = config_path {
        let config_contents = match fs::read_to_string(&config_path) {
            Ok(contents) => contents,
            Err(e) => {
                eprintln!("{}", e.to_string());
                std::process::exit(1);
            }
        };

        let unresolved_config: HashMap<String, String> =
            match serde_json::from_str(&config_contents) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{}", e.to_string());
                    std::process::exit(1);
                }
            };

        // Currently only TypeScript config is supported in file; it also
        // includes and understands all options provided in "global config".
        // More info: https://github.com/dsherret/dprint/pull/162#discussion_r399403808
        let global_config_result =
            dprint_core::configuration::resolve_global_config(&HashMap::new());

        if !global_config_result.diagnostics.is_empty() {
            for diagnostic in &global_config_result.diagnostics {
                eprintln!("{}", diagnostic.message);
            }
            std::process::exit(1);
        }

        let config_result =
            dprint_ts::configuration::resolve_config(&unresolved_config, &global_config_result.config);

        if !config_result.diagnostics.is_empty() {
            for diagnostic in &config_result.diagnostics {
                eprintln!("{}", diagnostic.message);
            }
            std::process::exit(1);
        }

        config_result.config
    } else {
        ConfigurationBuilder::new().build()
    }
}

fn resolve_file_paths<'a>(files: Option<Values<'a>>) -> Vec<PathBuf> {
    files
        .unwrap()
        .map(std::string::ToString::to_string)
        .map(PathBuf::from)
        .filter(|p| is_supported(p))
        .collect()
}

fn create_cli_parser<'a, 'b>() -> clap::App<'a, 'b> {
    App::new("dprint")
        .about("Format source files")
        .long_about(
            "Auto-format JavaScript/TypeScript source code.

  dprint myfile1.ts myfile2.ts

  dprint --check myfile1.ts myfile2.ts

  dprint --config dprint.conf.json myfile1.ts myfile2.ts",
        )
        .arg(
            Arg::with_name("check")
                .long("check")
                .help("Check if the source files are formatted.")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("config")
                .long("config")
                .help("Path to JSON configuration file.")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("files")
                .help("List of file paths to format")
                .takes_value(true)
                .multiple(true)
                .required(true),
        )
}

fn is_supported(path: &Path) -> bool {
    if let Some(ext) = path.extension() {
        if ext == "ts" || ext == "tsx" || ext == "js" || ext == "jsx" {
            true
        } else {
            false
        }
    } else {
        false
    }
}

#[test]
fn test_is_supported() {
    assert!(!is_supported(Path::new("tests/sub/dir")));
    assert!(!is_supported(Path::new("README.md")));
    assert!(is_supported(Path::new("lib/typescript.d.ts")));
    assert!(is_supported(Path::new("some/dir/001_hello.js")));
    assert!(is_supported(Path::new("some/dir/002_hello.ts")));
    assert!(is_supported(Path::new("foo.jsx")));
    assert!(is_supported(Path::new("foo.tsx")));
}
