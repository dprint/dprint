use anyhow::bail;
use anyhow::Result;
use clap::ArgMatches;

use crate::utils::StdInReader;

pub struct CliArgs {
  pub sub_command: SubCommand,
  pub verbose: bool,
  pub plugins: Vec<String>,
  pub config: Option<String>,
}

impl CliArgs {
  #[cfg(test)]
  pub fn empty() -> Self {
    Self {
      sub_command: SubCommand::Help("".to_string()),
      verbose: false,
      plugins: vec![],
      config: None,
    }
  }

  pub fn is_stdout_machine_readable(&self) -> bool {
    // these output json or other text that's read by stdout
    matches!(
      self.sub_command,
      SubCommand::StdInFmt(..) | SubCommand::EditorInfo | SubCommand::OutputResolvedConfig
    )
  }

  fn new_with_sub_command(sub_command: SubCommand) -> CliArgs {
    CliArgs {
      sub_command,
      verbose: false,
      config: None,
      plugins: Vec::new(),
    }
  }
}

#[derive(Debug, PartialEq)]
pub enum SubCommand {
  Check(CheckSubCommand),
  Fmt(FmtSubCommand),
  Config(ConfigSubCommand),
  ClearCache,
  OutputFilePaths(OutputFilePathsSubCommand),
  OutputResolvedConfig,
  OutputFormatTimes(OutputFormatTimesSubCommand),
  Version,
  License,
  Help(String),
  EditorInfo,
  EditorService(EditorServiceSubCommand),
  StdInFmt(StdInFmtSubCommand),
  #[cfg(target_os = "windows")]
  Hidden(HiddenSubCommand),
}

#[derive(Debug, PartialEq)]
pub struct CheckSubCommand {
  pub patterns: FilePatternArgs,
  pub incremental: bool,
}

#[derive(Debug, PartialEq)]
pub struct FmtSubCommand {
  pub diff: bool,
  pub patterns: FilePatternArgs,
  pub incremental: bool,
}

#[derive(Debug, PartialEq)]
pub enum ConfigSubCommand {
  Init,
  Update,
  Add(Option<String>),
}

#[derive(Debug, PartialEq)]
pub struct OutputFilePathsSubCommand {
  pub patterns: FilePatternArgs,
}

#[derive(Debug, PartialEq)]
pub struct OutputFormatTimesSubCommand {
  pub patterns: FilePatternArgs,
}

#[derive(Debug, PartialEq)]
pub struct EditorServiceSubCommand {
  pub parent_pid: u32,
}

#[derive(Debug, PartialEq)]
pub struct StdInFmtSubCommand {
  pub file_name_or_path: String,
  pub file_text: String,
  pub patterns: FilePatternArgs,
}

#[derive(Debug, PartialEq)]
#[cfg(target_os = "windows")]
pub enum HiddenSubCommand {
  #[cfg(target_os = "windows")]
  WindowsInstall(String),
  #[cfg(target_os = "windows")]
  WindowsUninstall(String),
}

#[derive(Debug, Default, PartialEq)]
pub struct FilePatternArgs {
  pub file_patterns: Vec<String>,
  pub exclude_file_patterns: Vec<String>,
  pub allow_node_modules: bool,
}

pub fn parse_args<TStdInReader: StdInReader>(args: Vec<String>, std_in_reader: TStdInReader) -> Result<CliArgs> {
  // this is all done because clap doesn't output exactly how I like
  if args.len() == 1 || (args.len() == 2 && (args[1] == "help" || args[1] == "--help")) {
    let mut help_text = Vec::new();
    let mut cli_parser = create_cli_parser(/* is outputting help */ true);
    cli_parser.try_get_matches_from_mut(vec![""])?;
    cli_parser.write_help(&mut help_text).unwrap();
    return Ok(CliArgs::new_with_sub_command(SubCommand::Help(String::from_utf8(help_text).unwrap())));
  } else if args.len() == 2 && (args[1] == "-v" || args[1] == "--version") {
    return Ok(CliArgs::new_with_sub_command(SubCommand::Version));
  }

  let cli_parser = create_cli_parser(false);
  let matches = match cli_parser.try_get_matches_from(&args) {
    Ok(result) => result,
    Err(err) => return Err(err.into()),
  };

  let sub_command = match matches.subcommand().unwrap() {
    ("fmt", matches) => {
      if let Some(file_name_path_or_extension) = matches.value_of("stdin").map(String::from) {
        let file_name_or_path = if file_name_path_or_extension.contains('.') {
          file_name_path_or_extension
        } else {
          // convert extension to file path
          format!("file.{}", file_name_path_or_extension)
        };
        SubCommand::StdInFmt(StdInFmtSubCommand {
          file_name_or_path,
          file_text: std_in_reader.read()?,
          patterns: parse_file_patterns(matches)?,
        })
      } else {
        SubCommand::Fmt(FmtSubCommand {
          diff: matches.is_present("diff"),
          patterns: parse_file_patterns(matches)?,
          incremental: parse_incremental(matches),
        })
      }
    }
    ("check", matches) => SubCommand::Check(CheckSubCommand {
      patterns: parse_file_patterns(matches)?,
      incremental: parse_incremental(matches),
    }),
    ("init", _) => SubCommand::Config(ConfigSubCommand::Init),
    ("config", matches) => SubCommand::Config(match matches.subcommand().unwrap() {
      ("init", _) => ConfigSubCommand::Init,
      ("add", matches) => ConfigSubCommand::Add(matches.value_of("url-or-plugin-name").map(ToOwned::to_owned)),
      ("update", _) => ConfigSubCommand::Update,
      _ => unreachable!(),
    }),
    ("clear-cache", _) => SubCommand::ClearCache,
    ("output-file-paths", matches) => SubCommand::OutputFilePaths(OutputFilePathsSubCommand {
      patterns: parse_file_patterns(matches)?,
    }),
    ("output-resolved-config", _) => SubCommand::OutputResolvedConfig,
    ("output-format-times", matches) => SubCommand::OutputFormatTimes(OutputFormatTimesSubCommand {
      patterns: parse_file_patterns(matches)?,
    }),
    ("version", _) => SubCommand::Version,
    ("license", _) => SubCommand::License,
    ("editor-info", _) => SubCommand::EditorInfo,
    ("editor-service", matches) => SubCommand::EditorService(EditorServiceSubCommand {
      parent_pid: matches.value_of("parent-pid").and_then(|v| v.parse::<u32>().ok()).unwrap(),
    }),
    #[cfg(target_os = "windows")]
    ("hidden", matches) => SubCommand::Hidden(match matches.subcommand().unwrap() {
      ("windows-install", matches) => HiddenSubCommand::WindowsInstall(matches.value_of("install-path").map(String::from).unwrap()),
      ("windows-uninstall", matches) => HiddenSubCommand::WindowsUninstall(matches.value_of("install-path").map(String::from).unwrap()),
      _ => unreachable!(),
    }),
    _ => {
      unreachable!();
    }
  };

  Ok(CliArgs {
    sub_command,
    verbose: matches.is_present("verbose"),
    config: matches.value_of("config").map(String::from),
    plugins: values_to_vec(matches.values_of("plugins")),
  })
}

fn parse_file_patterns(matches: &ArgMatches) -> Result<FilePatternArgs> {
  let plugins = values_to_vec(matches.values_of("plugins"));
  let file_patterns = values_to_vec(matches.values_of("files"));

  if !plugins.is_empty() && file_patterns.is_empty() {
    validate_plugin_args_when_no_files(&plugins)?;
  }

  Ok(FilePatternArgs {
    allow_node_modules: matches.is_present("allow-node-modules"),
    file_patterns,
    exclude_file_patterns: values_to_vec(matches.values_of("excludes")),
  })
}

fn parse_incremental(matches: &ArgMatches) -> bool {
  matches.is_present("incremental")
}

fn values_to_vec(values: Option<clap::Values>) -> Vec<String> {
  values.map(|x| x.map(std::string::ToString::to_string).collect()).unwrap_or_default()
}

/// Users have accidentally specified: dprint fmt --plugins <url1> <url2> -- <file-path>
/// But it should be: dprint fmt --plugins <url1> <url2> -- <file-path>
fn validate_plugin_args_when_no_files(plugins: &[String]) -> Result<()> {
  for (i, plugin) in plugins.iter().enumerate() {
    let lower_plugin = plugin.to_lowercase();
    let is_valid_plugin =
      lower_plugin.ends_with(".wasm") || lower_plugin.ends_with(".exe-plugin@") || lower_plugin.contains(".wasm@") || lower_plugin.contains(".exe-plugin");
    if !is_valid_plugin {
      let start_message = format!(
        "{} was specified as a plugin, but it doesn't look like one. Plugins must have a .wasm or .exe-plugin extension.",
        plugin
      );
      if i == 0 {
        bail!("{}", start_message);
      } else {
        bail!(
          "{}\n\nMaybe you meant to add two dashes after the plugins?\n  --plugins {} -- [file patterns go here...]",
          start_message,
          plugins[..i].join(" "),
        )
      }
    }
  }
  Ok(())
}

fn create_cli_parser<'a>(is_outputting_main_help: bool) -> clap::Command<'a> {
  use clap::AppSettings;
  use clap::Arg;
  use clap::Command;
  let app = Command::new("dprint");

  // hack to get this to display the way I want
  let app = if is_outputting_main_help {
    app.disable_help_subcommand(true).disable_version_flag(true).disable_help_flag(true)
  } else {
    app.subcommand_required(true)
  };

  app
    .setting(AppSettings::DeriveDisplayOrder)
    .bin_name("dprint")
    .version(env!("CARGO_PKG_VERSION"))
    .author("Copyright 2020-2022 by David Sherret")
    .about("Auto-formats source code based on the specified plugins.")
    .override_usage("dprint <SUBCOMMAND> [OPTIONS] [--] [file patterns]...")
    .help_template(r#"{bin} {version}
{author}

{about}

USAGE:
    {usage}

SUBCOMMANDS:
{subcommands}

More details at `dprint help <SUBCOMMAND>`

OPTIONS:
{options}

ENVIRONMENT VARIABLES:
  DPRINT_CACHE_DIR    The directory to store the dprint cache. Note that
                      this directory may be periodically deleted by the CLI.{after-help}"#)
    .after_help(
            r#"GETTING STARTED:
  1. Navigate to the root directory of a code repository.
  2. Run `dprint init` to create a dprint.json file in that directory.
  3. Modify configuration file if necessary.
  4. Run `dprint fmt` or `dprint check`.

EXAMPLES:
  Write formatted files to file system:

    dprint fmt

  Check for files that haven't been formatted:

    dprint check

  Specify path to config file other than the default:

    dprint fmt --config path/to/config/dprint.json

  Search for files using the specified file patterns:

    dprint fmt "**/*.{ts,tsx,js,jsx,json}""#,
    )
    .subcommand(
      Command::new("init")
        .about("Initializes a configuration file in the current directory.")
    )
    .subcommand(
      Command::new("fmt")
        .about("Formats the source files and writes the result to the file system.")
        .add_resolve_file_path_args()
        .add_incremental_arg()
        .arg(
          Arg::new("stdin")
            .long("stdin")
            .value_name("extension/file-name/file-path")
            .help("Format stdin and output the result to stdout. Provide an absolute file path to apply the inclusion and exclusion rules or an extension or file name to always format the text.")
            .required(false)
            .takes_value(true)
        )
        .arg(
          Arg::new("diff")
            .long("diff")
            .help("Outputs a check-like diff of every formatted file.")
            .takes_value(false)
            .required(false)
        )
    )
    .subcommand(
      Command::new("check")
        .about("Checks for any files that haven't been formatted.")
        .add_resolve_file_path_args()
        .add_incremental_arg()
    )
    .subcommand(
      Command::new("config")
        .about("Functionality related to the configuration file.")
        .subcommand_required(true)
        .subcommand(
          Command::new("init")
            .about("Initializes a configuration file in the current directory.")
        )
        .subcommand(
          Command::new("update")
            .about("Updates the plugins in the configuration file.")
        )
        .subcommand(
          Command::new("add")
            .about("Adds a plugin to the configuration file.")
            .arg(
              Arg::new("url-or-plugin-name")
                .required(false)
                .takes_value(true)
          )
        )
    )
    .subcommand(
      Command::new("output-file-paths")
        .about("Prints the resolved file paths for the plugins based on the args and configuration.")
        .add_resolve_file_path_args()
    )
    .subcommand(
      Command::new("output-resolved-config")
        .about("Prints the resolved configuration for the plugins based on the args and configuration.")
    )
    .subcommand(
      Command::new("output-format-times")
        .about("Prints the amount of time it takes to format each file. Use this for debugging.")
        .add_resolve_file_path_args()
    )
    .subcommand(
      Command::new("clear-cache")
        .about("Deletes the plugin cache directory.")
    )
    .subcommand(
      Command::new("license")
        .about("Outputs the software license.")
    )
    .subcommand(
      Command::new("editor-info")
        .hide(true)
    )
    .subcommand(
      Command::new("editor-service")
        .hide(true)
        .arg(
          Arg::new("parent-pid")
            .long("parent-pid")
            .required(true)
            .takes_value(true)
        )
    )
    .arg(
      Arg::new("config")
        .long("config")
        .short('c')
        .help("Path or url to JSON configuration file. Defaults to dprint.json or .dprint.json in current or ancestor directory when not provided.")
        .global(true)
        .takes_value(true),
    )
    .arg(
      Arg::new("plugins")
        .long("plugins")
        .value_name("urls/files")
        .help("List of urls or file paths of plugins to use. This overrides what is specified in the config file.")
        .global(true)
        .takes_value(true)
        .multiple_values(true)
    )
    .arg(
      Arg::new("verbose")
        .long("verbose")
        .help("Prints additional diagnostic information.")
        .global(true)
        .takes_value(false),
    )
    .subcommand(
      Command::new("hidden")
        .hide(true)
        .subcommand(
          Command::new("windows-install")
            .arg(
              Arg::new("install-path")
                .takes_value(true)
                .required(true)
            )
        )
        .subcommand(
          Command::new("windows-uninstall")
            .arg(
              Arg::new("install-path")
                .takes_value(true)
                .required(true)
            )
        )
    )
}

trait ClapExtensions {
  fn add_resolve_file_path_args(self) -> Self;
  fn add_incremental_arg(self) -> Self;
}

impl<'a> ClapExtensions for clap::Command<'a> {
  fn add_resolve_file_path_args(self) -> Self {
    use clap::Arg;
    self
      .arg(
        Arg::new("files")
          .help("List of file patterns in quotes to format. This overrides what is specified in the config file.")
          .takes_value(true)
          .multiple_values(true),
      )
      .arg(
        Arg::new("excludes")
          .long("excludes")
          .value_name("patterns")
          .help("List of file patterns or directories in quotes to exclude when formatting. This overrides what is specified in the config file.")
          .takes_value(true)
          .multiple_values(true),
      )
      .arg(
        Arg::new("allow-node-modules")
          .long("allow-node-modules")
          .help("Allows traversing node module directories (unstable - This flag will be renamed to be non-node specific in the future).")
          .takes_value(false),
      )
  }

  fn add_incremental_arg(self) -> Self {
    use clap::Arg;
    self.arg(
      Arg::new("incremental")
        .long("incremental")
        .help("Only format files when they change. This may alternatively be specified in the configuration file.")
        .takes_value(false),
    )
  }
}

#[cfg(test)]
mod test {
  use crate::utils::TestStdInReader;

  use super::*;

  #[test]
  fn plugins_with_file_paths_no_dash_at_first() {
    let err = test_args(vec!["fmt", "--plugins", "test", "other.ts"]).err().unwrap();
    assert_eq!(
      err.to_string(),
      concat!("test was specified as a plugin, but it doesn't look like one. Plugins must have a .wasm or .exe-plugin extension.")
    );
  }

  #[test]
  fn plugins_with_file_paths_no_dash_after_first() {
    let err = test_args(vec!["fmt", "--plugins", "https://plugins.dprint.dev/test.wasm", "other.ts"])
      .err()
      .unwrap();
    assert_eq!(
      err.to_string(),
      concat!(
        "other.ts was specified as a plugin, but it doesn't look like one. Plugins must have a .wasm or .exe-plugin extension.\n\n",
        "Maybe you meant to add two dashes after the plugins?\n",
        "  --plugins https://plugins.dprint.dev/test.wasm -- [file patterns go here...]",
      )
    );
  }

  fn test_args(args: Vec<&str>) -> Result<CliArgs> {
    let stdin_reader = TestStdInReader::default();
    let mut args: Vec<String> = args.into_iter().map(String::from).collect();
    args.insert(0, "".to_string());
    parse_args(args, stdin_reader)
  }
}
