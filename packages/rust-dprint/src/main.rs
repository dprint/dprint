extern crate dprint_core;
extern crate dprint_plugin_typescript as dprint_ts;

use clap::{App, Arg};
use dprint_ts::configuration::{Configuration, ConfigurationBuilder};
use serde_json;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

fn main() {
    let cli_parser = create_cli_parser();

    let matches = cli_parser.get_matches();

    let config = if let Some(config_path) = matches.value_of("config") {
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
    };

    let check = matches.is_present("check");
    let files: Vec<PathBuf> = matches
        .values_of("files")
        .unwrap()
        .map(std::string::ToString::to_string)
        .map(PathBuf::from)
        .filter(|p| is_supported(p))
        .collect();

    if let Err(e) = format(config, files, check) {
        eprintln!("{}", e.to_string());
        std::process::exit(1);
    }
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

fn check_source_files(config: Configuration, paths: Vec<PathBuf>) -> Result<(), std::io::Error> {
    let mut not_formatted_files = vec![];

    for file_path in paths {
        let file_path_str = file_path.to_string_lossy();
        let file_contents = fs::read_to_string(&file_path).unwrap();
        match dprint_ts::format_text(&file_path_str, &file_contents, &config) {
            Ok(None) => {
                // nothing to format, pass
            }
            Ok(Some(formatted_text)) => {
                if formatted_text != file_contents {
                    not_formatted_files.push(file_path);
                }
            }
            Err(e) => {
                eprintln!("Error checking: {}", &file_path_str);
                eprintln!("   {}", e);
            }
        }
    }

    if not_formatted_files.is_empty() {
        Ok(())
    } else {
        let f = if not_formatted_files.len() == 1 {
            "file"
        } else {
            "files"
        };
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Found {} not formatted {}", not_formatted_files.len(), f,),
        ))
    }
}

fn format_source_files(config: Configuration, paths: Vec<PathBuf>) -> Result<(), std::io::Error> {
    let mut not_formatted_files = vec![];

    for file_path in paths {
        let file_path_str = file_path.to_string_lossy();
        let file_contents = fs::read_to_string(&file_path)?;

        match dprint_ts::format_text(&file_path_str, &file_contents, &config) {
            Ok(None) => {
                // nothing to format, pass
            }
            Ok(Some(formatted_text)) => {
                if formatted_text != file_contents {
                    println!("{}", file_path_str);
                    fs::write(&file_path, formatted_text)?;
                    not_formatted_files.push(file_path);
                }
            }
            Err(e) => {
                eprintln!("Error formatting: {}", &file_path_str);
                eprintln!("   {}", e);
            }
        }
    }

    let f = if not_formatted_files.len() == 1 {
        "file"
    } else {
        "files"
    };
    println!("Formatted {} {}", not_formatted_files.len(), f);
    Ok(())
}

fn format(
    config: Configuration,
    target_files: Vec<PathBuf>,
    check: bool,
) -> Result<(), std::io::Error> {
    if check {
        check_source_files(config, target_files)?;
    } else {
        format_source_files(config, target_files)?;
    }
    Ok(())
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

#[test]
fn test_is_supported() {
    assert!(!is_supported(Path::new("tests/sub/dir")));
    assert!(!is_supported(Path::new("README.md")));
    assert!(!is_supported(Path::new("lib/typescript.d.ts")));
    assert!(is_supported(Path::new("some/dir/001_hello.js")));
    assert!(is_supported(Path::new("some/dir/002_hello.ts")));
    assert!(is_supported(Path::new("foo.jsx")));
    assert!(is_supported(Path::new("foo.tsx")));
}
