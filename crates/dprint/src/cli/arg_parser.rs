use clap::{App, Arg, Values, AppSettings};
use crate::types::ErrBox;

pub struct CliArgs {
    pub version: bool,
    pub init: bool,
    pub clear_cache: bool,
    pub output_file_paths: bool,
    pub output_resolved_config: bool,
    pub allow_node_modules: bool,
    pub verbose: bool,
    pub check: bool,
    pub config: Option<String>,
    pub file_patterns: Vec<String>,
    pub exclude_file_patterns: Vec<String>,
    pub plugin_urls: Vec<String>,
    pub help_text: Option<String>,
}

pub fn parse_args(args: Vec<String>) -> Result<CliArgs, ErrBox> {
    let mut cli_parser = create_cli_parser();
    let matches = match cli_parser.get_matches_from_safe_borrow(args) {
        Ok(result) => result,
        Err(err) => return err!("{}", err.to_string()),
    };
    let help_text = if matches.is_present("help") {
        let mut text = Vec::new();
        cli_parser.write_help(&mut text).unwrap();
        Some(String::from_utf8(text).unwrap())
    } else { None };

    Ok(CliArgs {
        version: matches.is_present("version"),
        init: matches.is_present("init"),
        clear_cache: matches.is_present("clear-cache"),
        output_file_paths: matches.is_present("output-file-paths"),
        output_resolved_config: matches.is_present("output-resolved-config"),
        check: matches.is_present("check"),
        verbose: matches.is_present("verbose"),
        allow_node_modules: matches.is_present("allow-node-modules"),
        config: matches.value_of("config").map(String::from),
        file_patterns: values_to_vec(matches.values_of("file patterns")),
        exclude_file_patterns: values_to_vec(matches.values_of("excludes")),
        plugin_urls: values_to_vec(matches.values_of("plugins")),
        help_text,
    })
}

fn create_cli_parser<'a, 'b>() -> clap::App<'a, 'b> {
    App::new("dprint")
        .setting(AppSettings::UnifiedHelpMessage)
        .setting(AppSettings::DisableHelpFlags)
        .bin_name("dprint")
        .version(env!("CARGO_PKG_VERSION"))
        .author("Copyright 2020 by David Sherret")
        .about("\nAuto-formats source code based on the specified plugins.")
        .after_help(
            r#"EXAMPLES:
    Create a dprint.config.json file:

      dprint --init

    Write formatted files to file system using the config file at ./dprint.config.json:

      dprint

    Check for any files that haven't been formatted:

      dprint --check

    Specify path to config file other than the default:

      dprint --config configs/dprint.config.json

    Write using the specified config and file paths:

      dprint --config formatting.config.json "**/*.{ts,tsx,js,jsx,json}""#,
        )
        .arg(
            Arg::with_name("check")
                .long("check")
                .help("Checks for any files that haven't been formatted.")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("config")
                .long("config")
                .short("c")
                .help("Path to JSON configuration file. Defaults to ./dprint.config.json when not provided.")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("file patterns")
                .help("List of patterns used to find files to format (globs in quotes separated by spaces). This overrides what is specified in the config file.")
                .takes_value(true)
                .multiple(true),
        )
        .arg(
            Arg::with_name("excludes")
                .long("excludes")
                .value_name("patterns")
                .help("List of patterns to exclude files or directories when formatting (globs in quotes separated by spaces). This overrides what is specified in the config file.")
                .takes_value(true)
                .multiple(true),
        )
        .arg(
            Arg::with_name("plugins")
                .long("plugins")
                .value_name("urls")
                .help("List of urls of plugins to use (urls separated by spaces). This overrides what is specified in the config file.")
                .takes_value(true)
                .multiple(true),
        )
        .arg(
            Arg::with_name("allow-node-modules")
                .long("allow-node-modules")
                .help("Allows traversing node module directories (unstable - This flag will be renamed to be non-node specific in the future).")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("init")
                .long("init")
                .help("Initializes a configuration file in the current directory.")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("clear-cache")
                .long("clear-cache")
                .help("Deletes the plugin cache directory.")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("version")
                .short("v")
                .long("version")
                .help("Prints the version.")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("output-resolved-config")
                .long("output-resolved-config")
                .help("Prints the resolved configuration.")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("output-file-paths")
                .long("output-file-paths")
                .help("Prints the resolved file paths.")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("verbose")
                .long("verbose")
                .help("Prints additional diagnostic information.")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("help")
                .long("help")
                .short("h")
                .hidden(true)
                .takes_value(false),
        )
}

fn values_to_vec(values: Option<Values>) -> Vec<String> {
    values.map(|x| x.map(std::string::ToString::to_string).collect()).unwrap_or(Vec::new())
}
