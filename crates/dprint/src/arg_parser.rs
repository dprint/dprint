use anyhow::bail;
use anyhow::Result;

use crate::utils::StdInReader;

pub struct CliArgs {
  pub sub_command: SubCommand,
  pub verbose: bool,
  pub plugins: Vec<String>,
  pub config: Option<String>,
  // It depends on the command whether these will exist... it
  // was just a lot easier to store these on a global object.
  pub incremental: bool,
  pub file_patterns: Vec<String>,
  pub exclude_file_patterns: Vec<String>,
  pub allow_node_modules: bool,
}

impl CliArgs {
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
      incremental: false,
      allow_node_modules: false,
      file_patterns: Vec::new(),
      exclude_file_patterns: Vec::new(),
    }
  }
}

#[derive(Debug, PartialEq)]
pub enum SubCommand {
  Check,
  Fmt(FmtSubCommand),
  Config(ConfigSubCommand),
  ClearCache,
  OutputFilePaths,
  OutputResolvedConfig,
  OutputFormatTimes,
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
pub struct FmtSubCommand {
  pub diff: bool,
}

#[derive(Debug, PartialEq)]
pub enum ConfigSubCommand {
  Init,
  Update,
  Add(Option<String>),
}

#[derive(Debug, PartialEq)]
pub struct EditorServiceSubCommand {
  pub parent_pid: u32,
}

#[derive(Debug, PartialEq)]
pub struct StdInFmtSubCommand {
  pub file_name_or_path: String,
  pub file_text: String,
}

#[derive(Debug, PartialEq)]
#[cfg(target_os = "windows")]
pub enum HiddenSubCommand {
  #[cfg(target_os = "windows")]
  WindowsInstall(String),
  #[cfg(target_os = "windows")]
  WindowsUninstall(String),
}

pub fn parse_args<TStdInReader: StdInReader>(args: Vec<String>, std_in_reader: TStdInReader) -> Result<CliArgs> {
  // this is all done because clap doesn't output exactly how I like
  if args.len() == 1 || (args.len() == 2 && (args[1] == "help" || args[1] == "--help")) {
    let mut help_text = Vec::new();
    let mut cli_parser = create_cli_parser(/* is outputting help */ true);
    cli_parser.get_matches_from_safe_borrow(vec![""])?;
    cli_parser.write_help(&mut help_text).unwrap();
    return Ok(CliArgs::new_with_sub_command(SubCommand::Help(String::from_utf8(help_text).unwrap())));
  } else if args.len() == 2 && (args[1] == "-v" || args[1] == "--version") {
    return Ok(CliArgs::new_with_sub_command(SubCommand::Version));
  }

  let cli_parser = create_cli_parser(false);
  let matches = match cli_parser.get_matches_from_safe(&args) {
    Ok(result) => result,
    Err(err) => bail!("{}", err.to_string()),
  };

  let sub_command = match matches.subcommand() {
    ("fmt", Some(matches)) => {
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
        })
      } else {
        SubCommand::Fmt(FmtSubCommand {
          diff: matches.is_present("diff"),
        })
      }
    }
    ("check", _) => SubCommand::Check,
    ("init", _) => SubCommand::Config(ConfigSubCommand::Init),
    ("config", Some(matches)) => SubCommand::Config(match matches.subcommand() {
      ("init", _) => ConfigSubCommand::Init,
      ("add", Some(matches)) => ConfigSubCommand::Add(matches.value_of("url-or-plugin-name").map(ToOwned::to_owned)),
      ("update", _) => ConfigSubCommand::Update,
      _ => unreachable!(),
    }),
    ("clear-cache", _) => SubCommand::ClearCache,
    ("output-file-paths", _) => SubCommand::OutputFilePaths,
    ("output-resolved-config", _) => SubCommand::OutputResolvedConfig,
    ("output-format-times", _) => SubCommand::OutputFormatTimes,
    ("version", _) => SubCommand::Version,
    ("license", _) => SubCommand::License,
    ("editor-info", _) => SubCommand::EditorInfo,
    ("editor-service", Some(matches)) => SubCommand::EditorService(EditorServiceSubCommand {
      parent_pid: matches.value_of("parent-pid").map(|v| v.parse::<u32>().ok()).flatten().unwrap(),
    }),
    #[cfg(target_os = "windows")]
    ("hidden", Some(matches)) => SubCommand::Hidden(match matches.subcommand() {
      ("windows-install", Some(matches)) => HiddenSubCommand::WindowsInstall(matches.value_of("install-path").map(String::from).unwrap()),
      ("windows-uninstall", Some(matches)) => HiddenSubCommand::WindowsUninstall(matches.value_of("install-path").map(String::from).unwrap()),
      _ => unreachable!(),
    }),
    _ => {
      unreachable!();
    }
  };
  let sub_command_matches = match matches.subcommand() {
    (_, Some(matches)) => Some(matches),
    _ => None,
  };

  let file_patterns = sub_command_matches.map(|m| values_to_vec(m.values_of("files"))).unwrap_or_default();
  let plugins = values_to_vec(matches.values_of("plugins"));

  if !plugins.is_empty() && file_patterns.is_empty() {
    validate_plugin_args_when_no_files(&plugins)?;
  }

  Ok(CliArgs {
    sub_command,
    verbose: matches.is_present("verbose"),
    config: matches.value_of("config").map(String::from),
    plugins,
    incremental: sub_command_matches.map(|m| m.is_present("incremental")).unwrap_or(false),
    allow_node_modules: sub_command_matches.map(|m| m.is_present("allow-node-modules")).unwrap_or(false),
    file_patterns,
    exclude_file_patterns: sub_command_matches.map(|m| values_to_vec(m.values_of("excludes"))).unwrap_or_default(),
  })
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
          "{}\n\nMaybe you meant to add two dashes after the plugins?\n  --plugins {} -- {} [etc...]",
          start_message,
          plugins[..i].join(" "),
          plugins[i],
        )
      }
    }
  }
  Ok(())
}

fn create_cli_parser<'a, 'b>(is_outputting_main_help: bool) -> clap::App<'a, 'b> {
  use clap::App;
  use clap::AppSettings;
  use clap::Arg;
  use clap::SubCommand;
  let app = App::new("dprint");

  // hack to get this to display the way I want
  let app = if is_outputting_main_help {
    app
      .setting(AppSettings::DisableHelpSubcommand)
      .setting(AppSettings::DisableHelpFlags)
      .setting(AppSettings::DisableVersion)
  } else {
    app.setting(AppSettings::SubcommandRequiredElseHelp)
  };

  app.setting(AppSettings::UnifiedHelpMessage)
    .setting(AppSettings::DeriveDisplayOrder)
    .bin_name("dprint")
    .version_short("v")
    .version(env!("CARGO_PKG_VERSION"))
    .author("Copyright 2020-2022 by David Sherret")
    .about("Auto-formats source code based on the specified plugins.")
    .usage("dprint <SUBCOMMAND> [OPTIONS] [--] [file patterns]...")
    // .help_about("Prints help information.") // todo: Enable once clap supports this as I want periods
    // .version_aboute("Prints the version.")
    .template(r#"{bin} {version}
{author}

{about}

USAGE:
    {usage}

SUBCOMMANDS:
{subcommands}

More details at `dprint help <SUBCOMMAND>`

OPTIONS:
{unified}

ENVIRONMENT VARIABLES:
  DPRINT_CACHE_DIR    The directory to store the dprint cache. Note that
                      this directory may be periodically deleted by the CLI.

{after-help}"#)
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
      SubCommand::with_name("init")
        .about("Initializes a configuration file in the current directory.")
    )
    .subcommand(
      SubCommand::with_name("fmt")
        .about("Formats the source files and writes the result to the file system.")
        .add_resolve_file_path_args()
        .add_incremental_arg()
        .arg(
          Arg::with_name("stdin")
            .long("stdin")
            .value_name("extension/file-name/file-path")
            .help("Format stdin and output the result to stdout. Provide an absolute file path to apply the inclusion and exclusion rules or an extension or file name to always format the text.")
            .required(false)
            .takes_value(true)
        )
        .arg(
          Arg::with_name("diff")
            .long("diff")
            .help("Outputs a check-like diff of every formatted file.")
            .takes_value(false)
            .required(false)
        )
    )
    .subcommand(
      SubCommand::with_name("check")
        .about("Checks for any files that haven't been formatted.")
        .add_resolve_file_path_args()
        .add_incremental_arg()
    )
    .subcommand(
      SubCommand::with_name("config")
        .about("Functionality related to the configuration file.")
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .subcommand(
          SubCommand::with_name("init")
            .about("Initializes a configuration file in the current directory.")
        )
        .subcommand(
          SubCommand::with_name("update")
            .about("Updates the plugins in the configuration file.")
        )
        .subcommand(
          SubCommand::with_name("add")
            .about("Adds a plugin to the configuration file.")
            .arg(
              Arg::with_name("url-or-plugin-name")
                .required(false)
                .takes_value(true)
          )
        )
    )
    .subcommand(
      SubCommand::with_name("output-file-paths")
        .about("Prints the resolved file paths for the plugins based on the args and configuration.")
        .add_resolve_file_path_args()
    )
    .subcommand(
      SubCommand::with_name("output-resolved-config")
        .about("Prints the resolved configuration for the plugins based on the args and configuration.")
    )
    .subcommand(
      SubCommand::with_name("output-format-times")
        .about("Prints the amount of time it takes to format each file. Use this for debugging.")
        .add_resolve_file_path_args()
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
      SubCommand::with_name("editor-service")
        .setting(AppSettings::Hidden)
        .arg(
          Arg::with_name("parent-pid")
            .long("parent-pid")
            .required(true)
            .takes_value(true)
        )
    )
    .arg(
      Arg::with_name("config")
        .long("config")
        .short("c")
        .help("Path or url to JSON configuration file. Defaults to dprint.json or .dprint.json in current or ancestor directory when not provided.")
        .global(true)
        .takes_value(true),
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
    .subcommand(
      SubCommand::with_name("hidden")
        .setting(AppSettings::Hidden)
        .subcommand(
          SubCommand::with_name("windows-install")
            .arg(
              Arg::with_name("install-path")
                .takes_value(true)
                .required(true)
            )
        )
        .subcommand(
          SubCommand::with_name("windows-uninstall")
            .arg(
              Arg::with_name("install-path")
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

impl<'a, 'b> ClapExtensions for clap::App<'a, 'b> {
  fn add_resolve_file_path_args(self) -> Self {
    use clap::Arg;
    self
      .arg(
        Arg::with_name("files")
          .help("List of file patterns in quotes to format. This overrides what is specified in the config file.")
          .takes_value(true)
          .multiple(true),
      )
      .arg(
        Arg::with_name("excludes")
          .long("excludes")
          .value_name("patterns")
          .help("List of file patterns or directories in quotes to exclude when formatting. This overrides what is specified in the config file.")
          .takes_value(true)
          .multiple(true),
      )
      .arg(
        Arg::with_name("allow-node-modules")
          .long("allow-node-modules")
          .help("Allows traversing node module directories (unstable - This flag will be renamed to be non-node specific in the future).")
          .takes_value(false),
      )
  }

  fn add_incremental_arg(self) -> Self {
    use clap::Arg;
    self.arg(
      Arg::with_name("incremental")
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
        "  --plugins https://plugins.dprint.dev/test.wasm -- other.ts [etc...]",
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
