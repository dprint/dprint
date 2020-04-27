use dprint_core::plugins::Formatter;
use clap::{App, Arg, Values};
use rayon::prelude::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use super::environment::Environment;
use super::create_formatter::create_formatter;

pub fn run_cli(environment: &impl Environment, args: Vec<String>) -> Result<(), String> {
    let cli_parser = create_cli_parser();
    let matches = match cli_parser.get_matches_from_safe(args) {
        Ok(result) => result,
        Err(err) => return Err(err.to_string()),
    };

    let formatter = create_formatter(matches.value_of("config"), environment)?;
    let check = matches.is_present("check");
    let files = resolve_file_paths(matches.values_of("files"));

    if check {
        check_files(environment, formatter, files)?
    } else {
        format_files(environment, formatter, files);
    }

    Ok(())
}

fn check_files(environment: &impl Environment, formatter: Formatter, paths: Vec<PathBuf>) -> Result<(), String> {
    let not_formatted_files_count = AtomicUsize::new(0);

    paths.par_iter().for_each(|file_path| {
        let file_contents = environment.read_file(&file_path);
        match file_contents {
            Ok(file_contents) => {
                match formatter.format_text(&file_path, &file_contents) {
                    Ok(Some(formatted_file_text)) => {
                        if formatted_file_text != file_contents {
                            not_formatted_files_count.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                    Ok(None) => {}, // do nothing
                    Err(e) => {
                        output_error(environment, &file_path, "Error checking", &e);
                    },
                }
            },
            Err(e) => {
                output_error(environment, &file_path, "Error reading file", &e);
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

fn format_files(environment: &impl Environment, formatter: Formatter, paths: Vec<PathBuf>) {
    let formatted_files_count = AtomicUsize::new(0);
    let files_count = paths.len();

    paths.par_iter().for_each(|file_path| {
        let file_contents = environment.read_file(&file_path);

        match file_contents {
            Ok(file_contents) => {
                match formatter.format_text(&file_path, &file_contents) {
                    Ok(Some(formatted_text)) => {
                        if formatted_text != file_contents {
                            environment.log(&file_path.to_string_lossy());
                            match environment.write_file(&file_path, &formatted_text) {
                                Ok(_) => {
                                    formatted_files_count.fetch_add(1, Ordering::SeqCst);
                                },
                                Err(e) => output_error(environment, &file_path, "Error writing file", &e),
                            };
                        }
                    }
                    Ok(None) => {}, // do nothing
                    Err(e) => output_error(environment, &file_path, "Error formatting", &e),
                }
            },
            Err(e) => output_error(environment, &file_path, "Error reading file", &e),
        }
    });

    let formatted_files_count = formatted_files_count.load(Ordering::SeqCst);
    let suffix = if files_count == 1 { "file" } else { "files" };
    environment.log(&format!("Formatted {} {}", formatted_files_count, suffix));
}

fn output_error(environment: &impl Environment, file_path: &PathBuf, text: &str, error: &impl std::fmt::Display) {
    environment.log_error(&format!("{}: {}\n    {}", text, &file_path.to_string_lossy(), error));
}

fn resolve_file_paths<'a>(files: Option<Values<'a>>) -> Vec<PathBuf> {
    files
        .unwrap()
        .map(std::string::ToString::to_string)
        .map(PathBuf::from)
        .collect()
}

fn create_cli_parser<'a, 'b>() -> clap::App<'a, 'b> {
    App::new("dprint")
        .about("Format source files")
        .long_about(
            "Auto-format JavaScript/TypeScript source code.

  dprint myfile1.ts myfile2.ts

  dprint --check myfile1.ts myfile2.ts

  dprint --config dprint.config.json myfile1.ts myfile2.ts",
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
