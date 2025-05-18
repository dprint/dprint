use anyhow::bail;
use anyhow::Result;
use clap::ArgMatches;
use thiserror::Error;

use crate::environment::Environment;
use crate::utils::LogLevel;
use crate::utils::StdInReader;

#[derive(Debug, Clone, Copy)]
pub enum ConfigDiscovery {
  True,
  False,
}

impl std::str::FromStr for ConfigDiscovery {
  type Err = String;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    match s.to_ascii_lowercase().as_str() {
      "true" => Ok(ConfigDiscovery::True),
      "false" => Ok(ConfigDiscovery::False),
      _ => Err(format!("expected 'true' or 'false', got '{s}'")),
    }
  }
}

impl ConfigDiscovery {
  pub fn is_true(&self) -> bool {
    matches!(self, Self::True)
  }
}

pub struct CliArgs {
  pub sub_command: SubCommand,
  pub log_level: LogLevel,
  pub plugins: Vec<String>,
  pub config: Option<String>,
  config_discovery: Option<ConfigDiscovery>,
}

impl CliArgs {
  #[cfg(test)]
  pub fn empty() -> Self {
    Self {
      sub_command: SubCommand::Help("".to_string()),
      log_level: LogLevel::Info,
      plugins: vec![],
      config: None,
      config_discovery: None,
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
      log_level: LogLevel::Info,
      config: None,
      plugins: Vec::new(),
      config_discovery: None,
    }
  }

  pub fn config_discovery(&self, env: &impl Environment) -> ConfigDiscovery {
    match self.config_discovery {
      Some(value) => value,
      None => match env.env_var("DPRINT_CONFIG_DISCOVERY") {
        Some(value) if value == "true" || value == "1" => ConfigDiscovery::True,
        Some(value) if value == "false" || value == "0" => ConfigDiscovery::False,
        _ => ConfigDiscovery::True,
      },
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
  Lsp,
  StdInFmt(StdInFmtSubCommand),
  Completions(clap_complete::Shell),
  Upgrade,
  #[cfg(target_os = "windows")]
  Hidden(HiddenSubCommand),
}
impl SubCommand {
  pub fn allow_no_files(&self) -> bool {
    match self {
      SubCommand::Check(a) => a.allow_no_files,
      SubCommand::Fmt(a) => a.allow_no_files,
      SubCommand::OutputFormatTimes(a) => a.allow_no_files,
      _ => false,
    }
  }

  pub fn file_patterns(&self) -> Option<&FilePatternArgs> {
    match self {
      SubCommand::Check(a) => Some(&a.patterns),
      SubCommand::Fmt(a) => Some(&a.patterns),
      SubCommand::StdInFmt(a) => Some(&a.patterns),
      SubCommand::OutputFilePaths(a) => Some(&a.patterns),
      SubCommand::OutputFormatTimes(a) => Some(&a.patterns),
      SubCommand::Config(_)
      | SubCommand::ClearCache
      | SubCommand::OutputResolvedConfig
      | SubCommand::Version
      | SubCommand::License
      | SubCommand::Help(_)
      | SubCommand::Lsp
      | SubCommand::EditorInfo
      | SubCommand::EditorService(_)
      | SubCommand::Completions(_)
      | SubCommand::Upgrade => None,
      #[cfg(target_os = "windows")]
      SubCommand::Hidden(_) => None,
    }
  }
}

#[derive(Debug, PartialEq, Eq)]
pub struct CheckSubCommand {
  pub patterns: FilePatternArgs,
  pub incremental: Option<bool>,
  pub list_different: bool,
  pub allow_no_files: bool,
  pub only_staged: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct FmtSubCommand {
  pub diff: bool,
  pub patterns: FilePatternArgs,
  pub incremental: Option<bool>,
  pub enable_stable_format: bool,
  pub allow_no_files: bool,
  pub only_staged: bool,
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
  pub allow_no_files: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct EditorServiceSubCommand {
  pub parent_pid: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub struct StdInFmtSubCommand {
  pub file_name_or_path: String,
  pub file_bytes: Vec<u8>,
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
  pub include_patterns: Vec<String>,
  pub include_pattern_overrides: Option<Vec<String>>,
  pub exclude_patterns: Vec<String>,
  pub exclude_pattern_overrides: Option<Vec<String>>,
  pub allow_node_modules: bool,
  pub only_staged: bool,
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
          file_bytes: std_in_reader.read()?,
          patterns: parse_file_patterns(matches)?,
        })
      } else {
        SubCommand::Fmt(FmtSubCommand {
          diff: matches.get_flag("diff"),
          patterns: parse_file_patterns(matches)?,
          incremental: parse_incremental(matches),
          enable_stable_format: !matches.get_flag("skip-stable-format"),
          allow_no_files: if matches.get_flag("staged") {
            true
          } else {
            matches.get_flag("allow-no-files")
          },
          only_staged: matches.get_flag("staged"),
        })
      }
    }
    ("check", matches) => SubCommand::Check(CheckSubCommand {
      patterns: parse_file_patterns(matches)?,
      incremental: parse_incremental(matches),
      only_staged: matches.get_flag("staged"),
      list_different: matches.get_flag("list-different"),
      allow_no_files: matches.get_flag("allow-no-files"),
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
      allow_no_files: matches.get_flag("allow-no-files"),
    }),
    ("version", _) => SubCommand::Version,
    ("license", _) => SubCommand::License,
    ("editor-info", _) => SubCommand::EditorInfo,
    ("editor-service", matches) => SubCommand::EditorService(EditorServiceSubCommand {
      parent_pid: matches.get_one::<String>("parent-pid").and_then(|v| v.parse::<u32>().ok()).unwrap(),
    }),
    ("lsp", _) => SubCommand::Lsp,
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
    log_level: if matches.get_flag("verbose") {
      LogLevel::Debug
    } else if let Some(log_level) = matches.get_one::<String>("log-level") {
      match log_level.as_str() {
        "debug" => LogLevel::Debug,
        "info" => LogLevel::Info,
        "warn" => LogLevel::Warn,
        "error" => LogLevel::Error,
        "silent" => LogLevel::Silent,
        _ => unreachable!(),
      }
    } else {
      LogLevel::Info
    },
    config: matches.get_one::<String>("config").map(String::from),
    config_discovery: matches.get_one::<ConfigDiscovery>("config-discovery").copied(),
    plugins: maybe_values_to_vec(matches.get_many("plugins")),
  })
}

fn parse_file_patterns(matches: &ArgMatches) -> Result<FilePatternArgs> {
  let plugins = maybe_values_to_vec(matches.get_many("plugins"));
  let file_patterns = maybe_values_to_vec(matches.get_many("files"));

  if !plugins.is_empty() && file_patterns.is_empty() {
    validate_plugin_args_when_no_files(&plugins)?;
  }

  Ok(FilePatternArgs {
    only_staged: matches.get_flag("staged"),
    allow_node_modules: matches.get_flag("allow-node-modules"),
    include_patterns: file_patterns,
    include_pattern_overrides: matches.get_many("includes-override").map(values_to_vec),
    exclude_patterns: maybe_values_to_vec(matches.get_many("excludes")),
    exclude_pattern_overrides: matches.get_many("excludes-override").map(values_to_vec),
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

fn maybe_values_to_vec(values: Option<clap::parser::ValuesRef<String>>) -> Vec<String> {
  values.map(values_to_vec).unwrap_or_default()
}

fn values_to_vec(values: clap::parser::ValuesRef<String>) -> Vec<String> {
  values.map(std::string::ToString::to_string).collect()
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
    .author("Copyright 2019 by David Sherret")
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
  DPRINT_CACHE_DIR     Directory to store the dprint cache. Note that this
                       directory may be periodically deleted by the CLI.
  DPRINT_MAX_THREADS   Limit the number of threads dprint uses for
                       formatting (ex. DPRINT_MAX_THREADS=4).
  DPRINT_CONFIG_DISCOVERY
                       Sets the config discovery mode. Set to "false"/"0" to disable.
  DPRINT_CERT          Load certificate authority from PEM encoded file.
  DPRINT_TLS_CA_STORE  Comma-separated list of order dependent certificate stores.
                       Possible values: "mozilla" and "system".
                       Defaults to "mozilla,system".
  DPRINT_IGNORE_CERTS  Unsafe way to get dprint to ignore certificates. Specify 1
                       to ignore all certificates or a comma separated list of specific
                       hosts to ignore (ex. dprint.dev,localhost,[::],127.0.0.1)
  HTTPS_PROXY          Proxy to use when downloading plugins or configuration
                       files (also supports HTTP_PROXY and NO_PROXY).{after-help}"#)
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
        .add_only_staged_arg()
        .add_allow_no_files_arg()
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
        .add_allow_no_files_arg()
        .add_only_staged_arg()
        .arg(
          Arg::new("list-different")
            .long("list-different")
            .help("Only outputs file paths that aren't formatted and doesn't output diffs.")
            .num_args(0)
        )
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
        .add_only_staged_arg()
    )
    .subcommand(
      Command::new("output-resolved-config")
        .about("Prints the resolved configuration for the plugins based on the args and configuration.")
    )
    .subcommand(
      Command::new("output-format-times")
        .about("Prints the amount of time it takes to format each file. Use this for debugging.")
        .add_resolve_file_path_args()
        .add_allow_no_files_arg()
        .add_only_staged_arg()
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
    .subcommand(
      Command::new("lsp")
      .about("Starts up a language server for formatting files.")
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
      Arg::new("config-discovery")
        .long("config-discovery")
        .help("Sets the config discovery mode. Set to `false` to completely disable.")
        .global(true)
        .value_parser(clap::value_parser!(ConfigDiscovery))
        .value_name("BOOLEAN")
        .num_args(1)
        .require_equals(true)
        .default_missing_value("true")
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
      Arg::new("log-level")
        .short('L')
        .long("log-level")
        .help("Set log level")
        .value_parser(["debug", "info", "warn", "error", "silent"])
        .default_value("info")
        .global(true),
    )
    .arg(
      Arg::new("verbose")
        .long("verbose")
        .help("Alias for --log-level=debug")
        .hide(true)
        .global(true)
        .num_args(0)
        .conflicts_with("log-level")
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
  fn add_allow_no_files_arg(self) -> Self;
  fn add_only_staged_arg(self) -> Self;
}

impl ClapExtensions for clap::Command {
  fn add_resolve_file_path_args(self) -> Self {
    use clap::Arg;
    self
      .arg(
        Arg::new("files")
          .help("List of file patterns in quotes to format. This can be a subset of what is found in the config file.")
          .num_args(1..),
      )
      .arg(
        Arg::new("includes-override")
          .long("includes-override")
          .value_name("patterns")
          .help("List of file patterns in quotes to format. This overrides what is specified in the config file.")
          .num_args(1..),
      )
      .arg(
        Arg::new("excludes")
          .long("excludes")
          .value_name("patterns")
          .help("List of file patterns or directories in quotes to exclude when formatting. This excludes in addition to what is found in the config file.")
          .num_args(1..),
      )
      .arg(
        Arg::new("excludes-override")
          .long("excludes-override")
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

  fn add_allow_no_files_arg(self) -> Self {
    use clap::Arg;
    self.arg(
      Arg::new("allow-no-files")
        .long("allow-no-files")
        .help("Causes dprint to exit with exit code 0 when no files are found instead of exit code 14.")
        .num_args(0)
        .required(false),
    )
  }

  fn add_only_staged_arg(self) -> Self {
    use clap::Arg;
    self.arg(
      Arg::new("staged")
        .long("staged")
        .help("Format only the staged files.")
        .num_args(0)
        .required(false),
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

  #[test]
  fn staged_arg() {
    let fmt_cmd = parse_fmt_sub_command(vec!["fmt"]).unwrap();
    assert_eq!(fmt_cmd.only_staged, false);
    let fmt_cmd = parse_fmt_sub_command(vec!["fmt", "--staged"]).unwrap();
    assert_eq!(fmt_cmd.only_staged, true);
  }

  #[test]
  fn no_files_arg() {
    let fmt_cmd = parse_fmt_sub_command(vec!["fmt", "--staged"]).unwrap();
    assert_eq!(fmt_cmd.only_staged, true);
    assert_eq!(fmt_cmd.allow_no_files, true);
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
