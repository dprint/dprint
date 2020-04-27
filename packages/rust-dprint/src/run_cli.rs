use dprint_core::plugins::Formatter;
use clap::{App, Arg, Values};
use rayon::prelude::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use super::environment::Environment;
use super::create_formatter::{create_formatter, get_uninitialized_plugins};

pub fn run_cli(environment: &impl Environment, args: Vec<String>) -> Result<(), String> {
    let cli_parser = create_cli_parser();
    let matches = match cli_parser.get_matches_from_safe(args) {
        Ok(result) => result,
        Err(err) => return Err(err.to_string()),
    };

    if matches.is_present("version") {
        output_version(environment);
        return Ok(());
    }

    let formatter = create_formatter(matches.value_of("config"), environment)?;
    let files = resolve_file_paths(matches.values_of("files"));
    if matches.is_present("check") {
        check_files(environment, formatter, files)?
    } else {
        format_files(environment, formatter, files);
    }

    Ok(())
}

fn output_version(environment: &impl Environment) {
    environment.log(&format!("{} v{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")));
    for plugin in get_uninitialized_plugins().iter() {
        environment.log(&format!("{} v{}", plugin.name(), plugin.version()));
    }
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
        Err(format!("Found {} not formatted {}.", not_formatted_files_count, f))
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
    if formatted_files_count > 0 {
        let suffix = if files_count == 1 { "file" } else { "files" };
        environment.log(&format!("Formatted {} {}.", formatted_files_count, suffix));
    }
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
                .required_unless("version"),
        )
        .arg(
            Arg::with_name("version")
                .short("v")
                .long("version")
                .help("Outputs the version")
                .takes_value(false),
        )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use super::run_cli;
    use super::super::environment::{Environment, TestEnvironment};

    #[test]
    fn it_should_output_version() {
        let environment = TestEnvironment::new();
        run_cli(&environment, vec![String::from(""), String::from("--version")]).unwrap();
        let logged_messages = environment.get_logged_messages();
        assert_eq!(logged_messages[0], format!("dprint v{}", env!("CARGO_PKG_VERSION")));
        assert_eq!(logged_messages.len(), 3); // good enough
    }

    #[test]
    fn it_should_format_files() {
        let environment = TestEnvironment::new();
        let file_path = PathBuf::from("/file.ts");
        environment.write_file(&file_path, "const t=4;").unwrap();
        run_cli(&environment, vec![String::from(""), String::from("/file.ts")]).unwrap();
        assert_eq!(environment.get_logged_messages(), vec!["Formatted 1 file."]);
        assert_eq!(environment.get_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path).unwrap(), "const t = 4;\n");
    }

    #[test]
    fn it_should_format_files_with_config() {
        let environment = TestEnvironment::new();
        let file_path1 = PathBuf::from("/file1.ts");
        let file_path2 = PathBuf::from("/file2.ts");
        let config_file_path = PathBuf::from("/config.json");
        environment.write_file(&file_path1, "const t=4;").unwrap();
        environment.write_file(&file_path2, "log(   55    );").unwrap();
        environment.write_file(&config_file_path, r#"{ "projectType": "openSource", "typescript": { "semiColons": "asi" } }"#).unwrap();

        run_cli(&environment, vec![String::from(""), String::from("--config"), String::from("/config.json"), String::from("/file1.ts"), String::from("/file2.ts")]).unwrap();

        assert_eq!(environment.get_logged_messages(), vec!["Formatted 2 files."]);
        assert_eq!(environment.get_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "const t = 4\n");
        assert_eq!(environment.read_file(&file_path2).unwrap(), "log(55)\n");
    }

    #[test]
    fn it_should_not_output_when_no_files_need_formatting() {
        let environment = TestEnvironment::new();
        let file_path = PathBuf::from("/file.ts");
        environment.write_file(&file_path, "const t = 4;\n").unwrap();
        run_cli(&environment, vec![String::from(""), String::from("/file.ts")]).unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0);
        assert_eq!(environment.get_logged_errors().len(), 0);
    }

    #[test]
    fn it_should_not_output_when_no_files_need_formatting_for_check() {
        let environment = TestEnvironment::new();
        let file_path = PathBuf::from("/file.ts");
        environment.write_file(&file_path, "const t = 4;\n").unwrap();
        run_cli(&environment, vec![String::from(""), String::from("--check"), String::from("/file.ts")]).unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0);
        assert_eq!(environment.get_logged_errors().len(), 0);
    }

    #[test]
    fn it_should_output_when_a_file_need_formatting_for_check() {
        let environment = TestEnvironment::new();
        environment.write_file(&PathBuf::from("/file.ts"), "const t=4;").unwrap();
        let error_message = run_cli(&environment, vec![String::from(""), String::from("--check"), String::from("/file.ts")]).err().unwrap();
        assert_eq!(error_message, "Found 1 not formatted file.");
        assert_eq!(environment.get_logged_messages().len(), 0);
        assert_eq!(environment.get_logged_errors().len(), 0);
    }

    #[test]
    fn it_should_output_when_files_need_formatting_for_check() {
        let environment = TestEnvironment::new();
        environment.write_file(&PathBuf::from("/file1.ts"), "const t=4;").unwrap();
        environment.write_file(&PathBuf::from("/file2.ts"), "const t=4;").unwrap();
        let error_message = run_cli(&environment, vec![String::from(""), String::from("--check"), String::from("/file1.ts"), String::from("/file2.ts")]).err().unwrap();
        assert_eq!(error_message, "Found 2 not formatted files.");
        assert_eq!(environment.get_logged_messages().len(), 0);
        assert_eq!(environment.get_logged_errors().len(), 0);
    }
}
