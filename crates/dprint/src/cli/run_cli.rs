use crate::cli::ConfigSubCommand;
use dprint_core::types::ErrBox;
use std::sync::Arc;

use crate::cache::Cache;
use crate::environment::Environment;
use crate::plugins::{PluginPools, PluginResolver};

use super::commands;
use super::{CliArgs, SubCommand};

pub fn run_cli<TEnvironment: Environment>(
  args: &CliArgs,
  environment: &TEnvironment,
  cache: &Cache<TEnvironment>,
  plugin_resolver: &PluginResolver<TEnvironment>,
  plugin_pools: Arc<PluginPools<TEnvironment>>,
) -> Result<(), ErrBox> {
  match &args.sub_command {
    SubCommand::Help(help_text) => commands::output_help(&args, cache, environment, plugin_resolver, help_text),
    SubCommand::License => commands::output_license(&args, cache, environment, plugin_resolver),
    SubCommand::EditorInfo => commands::output_editor_info(&args, cache, environment, plugin_resolver),
    SubCommand::EditorService(cmd) => commands::run_editor_service(&args, cache, environment, plugin_resolver, plugin_pools, cmd),
    SubCommand::ClearCache => commands::clear_cache(environment),
    SubCommand::Config(cmd) => match cmd {
      ConfigSubCommand::Init => commands::init_config_file(environment, &args.config),
      ConfigSubCommand::Update => commands::update_plugins_config_file(&args, cache, environment, plugin_resolver),
    },
    SubCommand::Version => commands::output_version(environment),
    SubCommand::StdInFmt(cmd) => commands::stdin_fmt(cmd, args, environment, cache, plugin_resolver, plugin_pools),
    SubCommand::OutputResolvedConfig => commands::output_resolved_config(args, cache, environment, plugin_resolver),
    SubCommand::OutputFilePaths => commands::output_file_paths(args, environment, cache, plugin_resolver),
    SubCommand::OutputFormatTimes => commands::output_format_times(args, environment, cache, plugin_resolver, plugin_pools),
    SubCommand::Check => commands::check(args, environment, cache, plugin_resolver, plugin_pools),
    SubCommand::Fmt => commands::format(args, environment, cache, plugin_resolver, plugin_pools),
    #[cfg(target_os = "windows")]
    SubCommand::Hidden(hidden_command) => match hidden_command {
      super::HiddenSubCommand::WindowsInstall(install_path) => commands::handle_windows_install(environment, &install_path),
      super::HiddenSubCommand::WindowsUninstall(install_path) => commands::handle_windows_uninstall(environment, &install_path),
    },
  }
}
