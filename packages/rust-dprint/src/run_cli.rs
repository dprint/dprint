use std::collections::HashMap;
use dprint_core::plugins::Formatter;
use clap::{App, Arg, Values, ArgMatches};
use rayon::prelude::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use super::environment::Environment;
use super::configuration;
use super::configuration::{ConfigMap, ConfigMapValue};
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
    if matches.is_present("init") {
        init_config_file(environment)?;
        environment.log("Created dprint.config.json");
        return Ok(());
    }

    let mut config_map = deserialize_config_file(matches.value_of("config"), environment)?;
    check_project_type_diagnostic(&mut config_map, environment);
    let file_paths = resolve_file_paths(&mut config_map, &matches, environment)?;

    if matches.is_present("output-file-paths") {
        output_file_paths(file_paths.iter(), environment);
        return Ok(());
    }

    let formatter = create_formatter(config_map, environment)?;

    if matches.is_present("output-resolved-config") {
        output_resolved_config(&formatter, environment);
        return Ok(());
    }

    if matches.is_present("check") {
        check_files(environment, formatter, file_paths)?
    } else {
        format_files(environment, formatter, file_paths);
    }

    Ok(())
}

fn output_version(environment: &impl Environment) {
    environment.log(&format!("{} v{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")));
    for plugin in get_uninitialized_plugins().iter() {
        environment.log(&format!("{} v{}", plugin.name(), plugin.version()));
    }
}

fn output_file_paths<'a>(file_paths: impl Iterator<Item=&'a PathBuf>, environment: &impl Environment) {
    for file_path in file_paths {
        environment.log(&file_path.to_string_lossy())
    }
}

fn output_resolved_config(formatter: &Formatter, environment: &impl Environment) {
    for plugin in formatter.iter_plugins() {
        environment.log(&format!("{}: {}", plugin.config_keys().join("/"), plugin.get_resolved_config()));
    }
}

fn init_config_file(environment: &impl Environment) -> Result<(), String> {
    let config_file_path = PathBuf::from("./dprint.config.json");
    if !environment.path_exists(&config_file_path) {
        environment.write_file(&config_file_path, configuration::get_init_config_file_text())
    } else {
        Err(String::from("Configuration file 'dprint.config.json' already exists in current working directory."))
    }
}

fn check_files(environment: &impl Environment, formatter: Formatter, file_paths: Vec<PathBuf>) -> Result<(), String> {
    let not_formatted_files_count = AtomicUsize::new(0);

    file_paths.par_iter().for_each(|file_path| {
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

fn format_files(environment: &impl Environment, formatter: Formatter, file_paths: Vec<PathBuf>) {
    let formatted_files_count = AtomicUsize::new(0);
    let files_count = file_paths.len();

    file_paths.par_iter().for_each(|file_path| {
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

fn create_cli_parser<'a, 'b>() -> clap::App<'a, 'b> {
    App::new("dprint")
        .about("Format source files")
        .long_about(
            r#"Auto-format JavaScript, TypeScript, and JSON source code.

  dprint "**/*.{ts,tsx,js,jsx,json}"

  dprint --check myfile1.ts myfile2.ts

  dprint --config dprint.config.json"#,
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
                .short("c")
                .help("Path to JSON configuration file.")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("file patterns")
                .help("List of file patterns used to find files to format.")
                .takes_value(true)
                .multiple(true),
        )
        .arg(
            Arg::with_name("allow-node-modules")
                .long("allow-node-modules")
                .help("Allows traversing node module directories.")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("init")
                .long("init")
                .help("Initializes a configuration file in the current directory.")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("version")
                .short("v")
                .long("version")
                .help("Outputs the version.")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("output-resolved-config")
                .long("output-resolved-config")
                .help("Outputs the resolved configuration.")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("output-file-paths")
                .long("output-file-paths")
                .help("Outputs the resolved file paths.")
                .takes_value(false),
        )
}

fn check_project_type_diagnostic(config_map: &mut ConfigMap, environment: &impl Environment) {
    if !config_map.is_empty() {
        if let Some(diagnostic) = configuration::handle_project_type_diagnostic(config_map) {
            environment.log_error(&diagnostic.message);
        }
    }
}

fn deserialize_config_file(config_path: Option<&str>, environment: &impl Environment) -> Result<ConfigMap, String> {
    if let Some(config_path) = config_path {
        let config_file_text = match environment.read_file(&PathBuf::from(config_path)) {
            Ok(contents) => contents,
            Err(e) => return Err(e.to_string()),
        };

        let result = match configuration::deserialize_config(&config_file_text) {
            Ok(map) => map,
            Err(e) => return Err(format!("Error deserializing. {}", e.to_string())),
        };

        Ok(result)
    } else {
        Ok(HashMap::new())
    }
}

fn resolve_file_paths(config_map: &mut ConfigMap, args: &ArgMatches, environment: &impl Environment) -> Result<Vec<PathBuf>, String> {
    let mut file_patterns = get_config_file_patterns(config_map)?;
    file_patterns.extend(resolve_file_patterns_from_cli(args.values_of("file patterns")));
    if !args.is_present("allow-node-modules") {
        file_patterns.push(String::from("!**/node_modules/**/*"));
    }
    return environment.glob(&file_patterns);

    fn resolve_file_patterns_from_cli(cli_file_patterns: Option<Values>) -> Vec<String> {
        if let Some(file_patterns) = cli_file_patterns {
            file_patterns.map(std::string::ToString::to_string).collect()
        } else {
            Vec::new()
        }
    }

    fn get_config_file_patterns(config_map: &mut ConfigMap) -> Result<Vec<String>, String> {
        let mut patterns = Vec::new();
        patterns.extend(take_array_from_config_map(config_map, "includes")?);
        patterns.extend(
            take_array_from_config_map(config_map, "excludes")?
                .into_iter()
                .map(|exclude| if exclude.starts_with("!") { exclude } else { format!("!{}", exclude) })
        );
        return Ok(patterns);

        fn take_array_from_config_map(config_map: &mut ConfigMap, property_name: &str) -> Result<Vec<String>, String> {
            let mut result = Vec::new();
            if let Some(value) = config_map.remove(property_name) {
                match value {
                    ConfigMapValue::Vec(elements) => {
                        result.extend(elements);
                    },
                    _ => return Err(format!("Expected array in '{}' property.", property_name))
                }
            }
            Ok(result)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use super::run_cli;
    use super::super::environment::{Environment, TestEnvironment};
    use super::super::configuration::*;

    #[test]
    fn it_should_output_version() {
        let environment = TestEnvironment::new();
        run_cli(&environment, vec![String::from(""), String::from("--version")]).unwrap();
        let logged_messages = environment.get_logged_messages();
        assert_eq!(logged_messages[0], format!("dprint v{}", env!("CARGO_PKG_VERSION")));
        assert_eq!(logged_messages.len(), 3); // good enough
    }

    #[test]
    fn it_should_output_resolve_config() {
        let environment = TestEnvironment::new();
        run_cli(&environment, vec![String::from(""), String::from("--output-resolved-config")]).unwrap();
        let logged_messages = environment.get_logged_messages();
        assert_eq!(logged_messages[0].starts_with("typescript/javascript: {\n"), true); // good enough
        assert_eq!(logged_messages[1].starts_with("json/jsonc: {\n"), true);
        assert_eq!(logged_messages.len(), 2);
    }

    #[test]
    fn it_should_output_resolved_file_paths() {
        let environment = TestEnvironment::new();
        environment.write_file(&PathBuf::from("/file.ts"), "const t=4;").unwrap();
        environment.write_file(&PathBuf::from("/file2.ts"), "const t=4;").unwrap();
        run_cli(&environment, vec![String::from(""), String::from("--output-file-paths"), String::from("**/*.ts")]).unwrap();
        let mut logged_messages = environment.get_logged_messages();
        logged_messages.sort();
        assert_eq!(logged_messages, vec!["/file.ts", "/file2.ts"]);
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
    fn it_should_ignore_files_in_node_modules_by_default() {
        let environment = TestEnvironment::new();
        environment.write_file(&PathBuf::from("/node_modules/file.ts"), "const t=4;").unwrap();
        environment.write_file(&PathBuf::from("/test/node_modules/file.ts"), "const t=4;").unwrap();
        run_cli(&environment, vec![String::from(""), String::from("**/*.ts")]).unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0);
        assert_eq!(environment.get_logged_errors().len(), 0);
    }

    #[test]
    fn it_should_not_ignore_files_in_node_modules_when_allowed() {
        let environment = TestEnvironment::new();
        environment.write_file(&PathBuf::from("/node_modules/file.ts"), "const t=4;").unwrap();
        environment.write_file(&PathBuf::from("/test/node_modules/file.ts"), "const t=4;").unwrap();
        run_cli(&environment, vec![String::from(""), String::from("--allow-node-modules"), String::from("**/*.ts")]).unwrap();
        assert_eq!(environment.get_logged_messages(), vec![String::from("Formatted 2 files.")]);
        assert_eq!(environment.get_logged_errors().len(), 0);
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
    fn it_should_format_files_with_config_using_c() {
        let environment = TestEnvironment::new();
        let file_path1 = PathBuf::from("/file1.ts");
        let config_file_path = PathBuf::from("/config.json");
        environment.write_file(&file_path1, "const t=4;").unwrap();
        environment.write_file(&config_file_path, r#"{ "projectType": "openSource", "typescript": { "semiColons": "asi" } }"#).unwrap();

        run_cli(&environment, vec![String::from(""), String::from("-c"), String::from("/config.json"), String::from("/file1.ts")]).unwrap();

        assert_eq!(environment.get_logged_messages(), vec!["Formatted 1 file."]);
        assert_eq!(environment.get_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "const t = 4\n");
    }

    #[test]
    fn it_should_format_files_with_config_includes() {
        let environment = TestEnvironment::new();
        let file_path1 = PathBuf::from("/file1.ts");
        let file_path2 = PathBuf::from("/file2.ts");
        let config_file_path = PathBuf::from("/config.json");
        environment.write_file(&file_path1, "const t=4;").unwrap();
        environment.write_file(&file_path2, "log(   55    );").unwrap();
        environment.write_file(&config_file_path, r#"{
            "projectType": "openSource",
            "typescript": { "semiColons": "asi" },
            "includes": ["**/*.ts"]
        }"#).unwrap();

        run_cli(&environment, vec![String::from(""), String::from("--config"), String::from("/config.json")]).unwrap();

        assert_eq!(environment.get_logged_messages(), vec!["Formatted 2 files."]);
        assert_eq!(environment.get_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "const t = 4\n");
        assert_eq!(environment.read_file(&file_path2).unwrap(), "log(55)\n");
    }

    #[test]
    fn it_should_format_files_with_config_excludes() {
        let environment = TestEnvironment::new();
        let file_path1 = PathBuf::from("/file1.ts");
        let file_path2 = PathBuf::from("/file2.ts");
        let config_file_path = PathBuf::from("/config.json");
        environment.write_file(&file_path1, "const t=4;").unwrap();
        environment.write_file(&file_path2, "log(   55    );").unwrap();
        environment.write_file(&config_file_path, r#"{
            "projectType": "openSource",
            "typescript": { "semiColons": "asi" },
            "includes": ["**/*.ts"],
            "excludes": ["/file2.ts"]
        }"#).unwrap();

        run_cli(&environment, vec![String::from(""), String::from("--config"), String::from("/config.json")]).unwrap();

        assert_eq!(environment.get_logged_messages(), vec!["Formatted 1 file."]);
        assert_eq!(environment.get_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "const t = 4\n");
        assert_eq!(environment.read_file(&file_path2).unwrap(), "log(   55    );");
    }

    #[test]
    fn it_should_only_warn_when_missing_project_type() {
        let environment = TestEnvironment::new();
        let file_path1 = PathBuf::from("/file1.ts");
        let config_file_path = PathBuf::from("/config.json");
        environment.write_file(&file_path1, "const t=4;").unwrap();
        environment.write_file(&config_file_path, r#"{ "typescript": { "semiColons": "asi" } }"#).unwrap();
        run_cli(&environment, vec![String::from(""), String::from("-c"), String::from("/config.json"), String::from("/file1.ts")]).unwrap();
        assert_eq!(environment.get_logged_messages(), vec!["Formatted 1 file."]);
        assert_eq!(environment.get_logged_errors()[0].find("The 'projectType' property").is_some(), true);
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

    #[test]
    fn it_should_initialize() {
        let environment = TestEnvironment::new();
        run_cli(&environment, vec![String::from(""), String::from("--init")]).unwrap();
        assert_eq!(environment.get_logged_messages(), vec!["Created dprint.config.json"]);
        assert_eq!(environment.read_file(&PathBuf::from("./dprint.config.json")).unwrap(), get_init_config_file_text());
        // ensure this file doesn't need formatting
        assert_eq!(
            run_cli(&environment, vec![String::from(""), String::from("--check"), String::from("/dprint.config.json")]).err().is_none(),
            true
        );
    }

    #[test]
    fn it_should_error_when_config_file_exists_on_initialize() {
        let environment = TestEnvironment::new();
        environment.write_file(&PathBuf::from("./dprint.config.json"), "{}").unwrap();
        let error_message = run_cli(&environment, vec![String::from(""), String::from("--init")]).err().unwrap();
        assert_eq!(error_message, "Configuration file 'dprint.config.json' already exists in current working directory.");
    }
}
