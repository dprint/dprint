use anyhow::bail;
use anyhow::Result;
use clap::ArgMatches;
use thiserror::Error;

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
      SubCommand::StdInFmt(..) | SubCommand::EditorInfo | SubCommand::OutputResolvedConfig | SubCommand::Completions(..)
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

#[derive(Debug, PartialEq, Eq)]
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
  Completions(clap_complete::Shell),
  Upgrade,
  #[cfg(target_os = "windows")]
  Hidden(HiddenSubCommand),
}

#[derive(Debug, PartialEq, Eq)]
pub struct CheckSubCommand {
  pub patterns: FilePatternArgs,
  pub incremental: Option<bool>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct FmtSubCommand {
  pub diff: bool,
  pub patterns: FilePatternArgs,
  pub incremental: Option<bool>,
  pub enable_stable_format: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ConfigSubCommand {
  Init,
  Update { yes: bool },
  Add(Option<String>),
}

#[derive(Debug, PartialEq, Eq)]
pub struct OutputFilePathsSubCommand {
  pub patterns: FilePatternArgs,
}

#[derive(Debug, PartialEq, Eq)]
pub struct OutputFormatTimesSubCommand {
  pub patterns: FilePatternArgs,
}

#[derive(Debug, PartialEq, Eq)]
pub struct EditorServiceSubCommand {
  pub parent_pid: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub struct StdInFmtSubCommand {
  pub file_name_or_path: String,
  pub file_text: String,
  pub patterns: FilePatternArgs,
}

#[derive(Debug, PartialEq, Eq)]
#[cfg(target_os = "windows")]
pub enum HiddenSubCommand {
  #[cfg(target_os = "windows")]
  WindowsInstall(String),
  #[cfg(target_os = "windows")]
  WindowsUninstall(String),
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct FilePatternArgs {
  pub file_patterns: Vec<String>,
  pub exclude_file_patterns: Vec<String>,
  pub allow_node_modules: bool,
}

#[derive(Debug, Error)]
#[error(transparent)]
pub struct ParseArgsError(#[from] anyhow::Error);

pub fn parse_args<TStdInReader: StdInReader>(args: Vec<String>, std_in_reader: TStdInReader) -> Result<CliArgs, ParseArgsError> {
  inner_parse_args(args, std_in_reader).map_err(ParseArgsError)
}

fn inner_parse_args<TStdInReader: StdInReader>(args: Vec<String>, std_in_reader: TStdInReader) -> Result<CliArgs> {
  // this is all done because clap doesn't output exactly how I like
  if args.len() == 1 || (args.len() == 2 && (args[1] == "help" || args[1] == "--help")) {
    let mut cli_parser = create_cli_parser(CliArgParserKind::ForOutputtingMainHelp);
    cli_parser.try_get_matches_from_mut(vec![""])?;
    let help_text = format!("{}", cli_parser.render_help());
    return Ok(CliArgs::new_with_sub_command(SubCommand::Help(help_text)));
  } else if args.len() == 2 && (args[1] == "-v" || args[1] == "-V" || args[1] == "--version") {
    return Ok(CliArgs::new_with_sub_command(SubCommand::Version));
  }

  let cli_parser = create_cli_parser(CliArgParserKind::Default);
  let matches = match cli_parser.try_get_matches_from(&args) {
    Ok(result) => result,
    Err(err) => return Err(err.into()),
  };

  let sub_command = match matches.subcommand().unwrap() {
    ("fmt", matches) => {
      if let Some(file_name_path_or_extension) = matches.get_one::<String>("stdin").map(String::from) {
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
          diff: matches.get_flag("diff"),
          patterns: parse_file_patterns(matches)?,
          incremental: parse_incremental(matches),
          enable_stable_format: !matches.get_flag("skip-stable-format"),
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
      ("add", matches) => ConfigSubCommand::Add(matches.get_one::<String>("url-or-plugin-name").map(String::from)),
      ("update", matches) => ConfigSubCommand::Update {
        yes: *matches.get_one::<bool>("yes").unwrap(),
      },
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
      parent_pid: matches.get_one::<String>("parent-pid").and_then(|v| v.parse::<u32>().ok()).unwrap(),
    }),
    ("completions", matches) => SubCommand::Completions(matches.get_one::<clap_complete::Shell>("shell").unwrap().to_owned()),
    ("upgrade", _) => SubCommand::Upgrade,
    #[cfg(target_os = "windows")]
    ("hidden", matches) => SubCommand::Hidden(match matches.subcommand().unwrap() {
      ("windows-install", matches) => HiddenSubCommand::WindowsInstall(matches.get_one::<String>("install-path").map(String::from).unwrap()),
      ("windows-uninstall", matches) => HiddenSubCommand::WindowsUninstall(matches.get_one::<String>("install-path").map(String::from).unwrap()),
      _ => unreachable!(),
    }),
    _ => {
      unreachable!();
    }
  };

  Ok(CliArgs {
    sub_command,
    verbose: matches.get_flag("verbose"),
    config: matches.get_one::<String>("config").map(String::from),
    plugins: values_to_vec(matches.get_many("plugins")),
  })
}

fn parse_file_patterns(matches: &ArgMatches) -> Result<FilePatternArgs> {
  let plugins = values_to_vec(matches.get_many("plugins"));
  let file_patterns = values_to_vec(matches.get_many("files"));

  if !plugins.is_empty() && file_patterns.is_empty() {
    validate_plugin_args_when_no_files(&plugins)?;
  }

  Ok(FilePatternArgs {
    allow_node_modules: matches.get_flag("allow-node-modules"),
    file_patterns,
    exclude_file_patterns: values_to_vec(matches.get_many("excludes")),
  })
}

fn parse_incremental(matches: &ArgMatches) -> Option<bool> {
  if let Some(incremental) = matches.get_one::<String>("incremental") {
    Some(incremental != "false")
  } else if matches.contains_id("incremental") {
    Some(true)
  } else {
    None
  }
}

fn values_to_vec(values: Option<clap::parser::ValuesRef<String>>) -> Vec<String> {
  values.map(|x| x.map(std::string::ToString::to_string).collect()).unwrap_or_default()
}

/// Users have accidentally specified: dprint fmt --plugins <url1> <url2> -- <file-path>
/// But it should be: dprint fmt --plugins <url1> <url2> -- <file-path>
fn validate_plugin_args_when_no_files(plugins: &[String]) -> Result<()> {
  for (i, plugin) in plugins.iter().enumerate() {
    let lower_plugin = plugin.to_lowercase();
    let is_valid_plugin =
      lower_plugin.ends_with(".wasm") || lower_plugin.ends_with(".json") || lower_plugin.contains(".wasm@") || lower_plugin.contains(".json@");
    if !is_valid_plugin {
      let start_message = format!(
        "{} was specified as a plugin, but it doesn't look like one. Plugins must have a .wasm or .json extension.",
        plugin
      );
      if i == 0 {
        bail!("{}", start_message);
      } else {
        bail!(
          "{}\n\nMaybe you meant to add two dashes after the plugins?\n  --plugins {} -- [file patterns]...",
          start_message,
          plugins[..i].join(" "),
        )
      }
    }
  }
  Ok(())
}

#[derive(Default, PartialEq, Eq)]
pub enum CliArgParserKind {
  ForOutputtingMainHelp,
  ForCompletions,
  #[default]
  Default,
}

pub fn create_cli_parser(kind: CliArgParserKind) -> clap::Command {
  use clap::Arg;
  use clap::Command;

  let mut app = Command::new("dprint");

  // hack to get this to display the way I want
  app = if kind == CliArgParserKind::ForOutputtingMainHelp {
    app.disable_help_subcommand(true).disable_version_flag(true).disable_help_flag(true)
  } else {
    app.subcommand_required(true)
  };

  app = app
    .bin_name("dprint")
    .version(env!("CARGO_PKG_VERSION"))
    .author("Copyright 2020-2023 by David Sherret")
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
  DPRINT_CACHE_DIR    Directory to store the dprint cache. Note that this
                      directory may be periodically deleted by the CLI.
  DPRINT_MAX_THREADS  Limit the number of threads dprint uses for
                      formatting (ex. DPRINT_MAX_THREADS=4).
  HTTPS_PROXY         Proxy to use when downloading plugins or configuration
                      files (set HTTP_PROXY for HTTP).{after-help}"#)
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
            .num_args(1)
        )
        .arg(
          Arg::new("diff")
            .long("diff")
            .help("Outputs a check-like diff of every formatted file.")
            .num_args(0)
            .required(false)
        )
        .arg(
          Arg::new("skip-stable-format")
            .long("skip-stable-format")
            .help("Whether to skip formatting a file multiple times until the output is stable")
            // hidden because this needs more thought and probably shouldn't be allowed with incremental
            .hide(true)
            .num_args(0)
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
            .arg(Arg::new("yes").help("Upgrade process plugins without prompting to confirm checksums.").short('y').long("yes").action(clap::ArgAction::SetTrue))
        )
        .subcommand(
          Command::new("add")
            .about("Adds a plugin to the configuration file.")
            .arg(
              Arg::new("url-or-plugin-name")
                .required(false)
                .num_args(1)
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
      Command::new("upgrade")
        .about("Upgrades the dprint executable.")
    )
    .subcommand(
      Command::new("completions").about("Generate shell completions script for dprint").arg(
        Arg::new("shell")
          .action(clap::ArgAction::Set)
          .value_parser(clap::value_parser!(clap_complete::Shell))
          .required(true)
      )
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
            .num_args(1)
        )
    )
    .arg(
      Arg::new("config")
        .long("config")
        .short('c')
        .help("Path or url to JSON configuration file. Defaults to dprint.json(c) or .dprint.json(c) in current or ancestor directory when not provided.")
        .global(true)
        .num_args(1)
    )
    .arg(
      Arg::new("plugins")
        .long("plugins")
        .value_name("urls/files")
        .help("List of urls or file paths of plugins to use. This overrides what is specified in the config file.")
        .global(true)
        .num_args(1..)
    )
    .arg(
      Arg::new("verbose")
        .long("verbose")
        .help("Prints additional diagnostic information.")
        .global(true)
        .num_args(0)
    );

  #[cfg(target_os = "windows")]
  if kind == CliArgParserKind::Default {
    app = app.subcommand(
      Command::new("hidden")
        .hide(true)
        .subcommand(Command::new("windows-install").arg(Arg::new("install-path").num_args(1).required(true)))
        .subcommand(Command::new("windows-uninstall").arg(Arg::new("install-path").num_args(1).required(true))),
    );
  }

  app
}

trait ClapExtensions {
  fn add_resolve_file_path_args(self) -> Self;
  fn add_incremental_arg(self) -> Self;
}

impl ClapExtensions for clap::Command {
  fn add_resolve_file_path_args(self) -> Self {
    use clap::Arg;
    self
      .arg(
        Arg::new("files")
          .help("List of file patterns in quotes to format. This overrides what is specified in the config file.")
          .num_args(1..),
      )
      .arg(
        Arg::new("excludes")
          .long("excludes")
          .value_name("patterns")
          .help("List of file patterns or directories in quotes to exclude when formatting. This overrides what is specified in the config file.")
          .num_args(1..),
      )
      .arg(
        Arg::new("allow-node-modules")
          .long("allow-node-modules")
          .help("Allows traversing node module directories (unstable - This flag will be renamed to be non-node specific in the future).")
          .num_args(0),
      )
  }

  fn add_incremental_arg(self) -> Self {
    use clap::Arg;
    self.arg(
      Arg::new("incremental")
        .long("incremental")
        .help("Only format files when they change. This may alternatively be specified in the configuration file.")
        .num_args(0..=1)
        .value_parser(["true", "false"])
        .require_equals(true),
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
      concat!("test was specified as a plugin, but it doesn't look like one. Plugins must have a .wasm or .json extension.")
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
        "other.ts was specified as a plugin, but it doesn't look like one. Plugins must have a .wasm or .json extension.\n\n",
        "Maybe you meant to add two dashes after the plugins?\n",
        "  --plugins https://plugins.dprint.dev/test.wasm -- [file patterns]...",
      )
    );
  }

  #[test]
  fn version_flag() {
    assert_version(vec!["-v"]);
    assert_version(vec!["-V"]);
    assert_version(vec!["--version"]);
  }

  fn assert_version(args: Vec<&str>) {
    let args = test_args(args).unwrap();
    assert_eq!(args.sub_command, SubCommand::Version);
  }

  #[test]
  fn incremental_arg() {
    let fmt_cmd = parse_fmt_sub_command(vec!["fmt"]).unwrap();
    assert_eq!(fmt_cmd.incremental, None);
    let fmt_cmd = parse_fmt_sub_command(vec!["fmt", "--incremental=true"]).unwrap();
    assert_eq!(fmt_cmd.incremental, Some(true));
    let fmt_cmd = parse_fmt_sub_command(vec!["fmt", "--incremental=false"]).unwrap();
    assert_eq!(fmt_cmd.incremental, Some(false));
    let fmt_cmd = parse_fmt_sub_command(vec!["fmt", "--incremental"]).unwrap();
    assert_eq!(fmt_cmd.incremental, Some(true));
  }

  fn parse_fmt_sub_command(args: Vec<&str>) -> Result<FmtSubCommand, ParseArgsError> {
    let args = test_args(args)?;
    match args.sub_command {
      SubCommand::Fmt(cmd) => Ok(cmd),
      _ => unreachable!(),
    }
  }

  fn test_args(args: Vec<&str>) -> Result<CliArgs, ParseArgsError> {
    let stdin_reader = TestStdInReader::default();
    let mut args: Vec<String> = args.into_iter().map(String::from).collect();
    args.insert(0, "".to_string());
    parse_args(args, stdin_reader)
  }
}
