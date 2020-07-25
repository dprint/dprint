use dprint_core::types::ErrBox;
use super::StdInReader;

pub struct CliArgs {
    pub sub_command: SubCommand,
    pub allow_node_modules: bool,
    pub verbose: bool,
    pub incremental: bool,
    pub file_patterns: Vec<String>,
    pub exclude_file_patterns: Vec<String>,
    pub plugins: Vec<String>,
    pub config: Option<String>,
}

impl CliArgs {
    pub fn is_silent_output(&self) -> bool {
        match self.sub_command {
            SubCommand::EditorInfo | SubCommand::StdInFmt(..) => true,
            _ => false
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum SubCommand {
    Check,
    Fmt,
    Init,
    ClearCache,
    OutputFilePaths,
    OutputResolvedConfig,
    OutputFormatTimes,
    Version,
    License,
    Help(String),
    EditorInfo,
    StdInFmt(StdInFmt),
}

#[derive(Debug, PartialEq)]
pub struct StdInFmt {
    pub file_name: String,
    pub file_text: String,
}

pub fn parse_args<TStdInReader: StdInReader>(args: Vec<String>, std_in_reader: &TStdInReader) -> Result<CliArgs, ErrBox> {
    let mut cli_parser = create_cli_parser();
    let matches = match cli_parser.get_matches_from_safe_borrow(args) {
        Ok(result) => result,
        Err(err) => return err!("{}", err.to_string()),
    };

    let sub_command = if matches.is_present("fmt") {
        SubCommand::Fmt
    } else if matches.is_present("check") {
        SubCommand::Check
    } else if matches.is_present("init") {
        SubCommand::Init
    } else if matches.is_present("clear-cache") {
        SubCommand::ClearCache
    } else if matches.is_present("output-file-paths") {
        SubCommand::OutputFilePaths
    } else if matches.is_present("output-resolved-config") {
        SubCommand::OutputResolvedConfig
    } else if matches.is_present("output-format-times") {
        SubCommand::OutputFormatTimes
    } else if matches.is_present("version") {
        SubCommand::Version
    } else if matches.is_present("license") {
        SubCommand::License
    } else if matches.is_present("editor-info") {
        SubCommand::EditorInfo
    } else if matches.is_present("stdin-fmt") {
        let std_in_fmt_matches = match matches.subcommand_matches("stdin-fmt") {
            Some(matches) => matches,
            None => return err!("Could not find stdin-fmt subcommand matches."),
        };
        SubCommand::StdInFmt(StdInFmt {
            file_name: std_in_fmt_matches.value_of("file-name").map(String::from).unwrap(),
            file_text: std_in_reader.read()?,
        })
    } else {
        SubCommand::Help({
            let mut text = Vec::new();
            cli_parser.write_help(&mut text).unwrap();
            String::from_utf8(text).unwrap()
        })
    };

    Ok(CliArgs {
        sub_command,
        verbose: matches.is_present("verbose"),
        incremental: matches.is_present("incremental"),
        allow_node_modules: matches.is_present("allow-node-modules"),
        config: matches.value_of("config").map(String::from),
        file_patterns: values_to_vec(matches.values_of("files")),
        exclude_file_patterns: values_to_vec(matches.values_of("excludes")),
        plugins: values_to_vec(matches.values_of("plugins")),
    })
}

fn create_cli_parser<'a, 'b>() -> clap::App<'a, 'b> {
    use clap::{App, Arg, SubCommand, AppSettings};
    App::new("dprint")
        .setting(AppSettings::UnifiedHelpMessage)
        .setting(AppSettings::DisableHelpFlags)
        .setting(AppSettings::DisableHelpSubcommand)
        .setting(AppSettings::DeriveDisplayOrder)
        .bin_name("dprint")
        .version(env!("CARGO_PKG_VERSION"))
        .author("Copyright 2020 by David Sherret")
        .about("Auto-formats source code based on the specified plugins.")
        .usage("dprint <SUBCOMMAND> [OPTIONS] [--] [files]...")
        .template(r#"{bin} {version}
{author}

{about}

USAGE:
    {usage}

SUBCOMMANDS:
{subcommands}

OPTIONS:
{unified}

ARGS:
{positionals}

{after-help}"#)
        .after_help(
            r#"GETTING STARTED:
    1. Navigate to the root directory of a code repository.
    2. Run `dprint init` to create a .dprintrc.json file in that directory.
    3. Modify configuration file if necessary.
    4. Run `dprint fmt` or `dprint check`.

EXAMPLES:
    Write formatted files to file system:

      dprint fmt

    Check for files that haven't been formatted:

      dprint check

    Specify path to config file other than the default:

      dprint fmt --config path/to/config/.dprintrc.json

    Search for files using the specified file patterns:

      dprint fmt "**/*.{ts,tsx,js,jsx,json}""#,
        )
        .subcommand(
            SubCommand::with_name("init")
                .about("Initializes a configuration file in the current directory.")
        )
        .subcommand(
            SubCommand::with_name("fmt")
                .about("Formats the source files and writes the result to the file system.")
        )
        .subcommand(
            SubCommand::with_name("check")
                .about("Checks for any files that haven't been formatted.")
        )
        .subcommand(
            SubCommand::with_name("output-file-paths")
                .about("Prints the resolved file paths for the plugins based on the args and configuration.")
        )
        .subcommand(
            SubCommand::with_name("output-resolved-config")
                .about("Prints the resolved configuration for the plugins based on the args and configuration.")
        )
        .subcommand(
            SubCommand::with_name("output-format-times")
                .about("Prints the amount of time it takes to format each file. Use this for debugging.")
        )
        .subcommand(
            SubCommand::with_name("clear-cache")
                .about("Deletes the plugin cache directory.")
        )
        .subcommand(
            SubCommand::with_name("license")
                .about("Outputs the software license.")
        )
        .subcommand(
            SubCommand::with_name("editor-info")
                .setting(AppSettings::Hidden)
        )
        .subcommand(
            SubCommand::with_name("stdin-fmt")
                .setting(AppSettings::Hidden)
                .arg(
                    Arg::with_name("file-name")
                        .long("file-name")
                        .required(true)
                        .takes_value(true)
                )
        )
        .arg(
            Arg::with_name("files")
                .help("List of files or globs in quotes to format. This overrides what is specified in the config file.")
                .takes_value(true)
                .global(true)
                .conflicts_with("stdin-fmt")
                .multiple(true),
        )
        .arg(
            Arg::with_name("config")
                .long("config")
                .short("c")
                .help("Path or url to JSON configuration file. Defaults to .dprintrc.json in current or ancestor directory when not provided.")
                .global(true)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("help")
                .long("help")
                .short("h")
                .hidden(true)
                .takes_value(false),
        )
        .arg(
            Arg::with_name("excludes")
                .long("excludes")
                .global(true)
                .value_name("patterns")
                .help("List of files or directories or globs in quotes to exclude when formatting. This overrides what is specified in the config file.")
                .takes_value(true)
                .multiple(true),
        )
        .arg(
            Arg::with_name("allow-node-modules")
                .long("allow-node-modules")
                .help("Allows traversing node module directories (unstable - This flag will be renamed to be non-node specific in the future).")
                .global(true)
                .takes_value(false),
        )
        .arg(
            Arg::with_name("incremental")
                .long("incremental")
                .help("Only format files only when they change. This may alternatively be specified in the configuration file.")
                .global(true)
                .takes_value(false),
        )
        .arg(
            Arg::with_name("plugins")
                .long("plugins")
                .value_name("urls/files")
                .help("List of urls or file paths of plugins to use. This overrides what is specified in the config file.")
                .global(true)
                .takes_value(true)
                .multiple(true),
        )
        .arg(
            Arg::with_name("verbose")
                .long("verbose")
                .help("Prints additional diagnostic information.")
                .global(true)
                .takes_value(false),
        )
        .arg(
            Arg::with_name("version")
                .short("v")
                .long("version")
                .help("Prints the version.")
                .takes_value(false),
        )
}

fn values_to_vec(values: Option<clap::Values>) -> Vec<String> {
    values.map(|x| x.map(std::string::ToString::to_string).collect()).unwrap_or(Vec::new())
}
