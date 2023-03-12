use anyhow::Result;
use std::sync::Arc;

use crate::environment::Environment;
use crate::plugins::PluginResolver;
use crate::plugins::PluginsCollection;

use crate::arg_parser::CliArgs;
use crate::arg_parser::ConfigSubCommand;
use crate::arg_parser::SubCommand;
use crate::commands;

pub async fn run_cli<TEnvironment: Environment>(
  args: &CliArgs,
  environment: &TEnvironment,
  plugin_resolver: &PluginResolver<TEnvironment>,
  plugin_pools: Arc<PluginsCollection<TEnvironment>>,
) -> Result<()> {
  match &args.sub_command {
    SubCommand::Help(help_text) => commands::output_help(args, environment, plugin_resolver, help_text).await,
    SubCommand::License => commands::output_license(args, environment, plugin_resolver).await,
    SubCommand::EditorInfo => commands::output_editor_info(args, environment, plugin_resolver).await,
    SubCommand::EditorService(cmd) => commands::run_editor_service(args, environment, plugin_resolver, plugin_pools, cmd).await,
    SubCommand::ClearCache => commands::clear_cache(environment),
    SubCommand::Config(cmd) => match cmd {
      ConfigSubCommand::Init => commands::init_config_file(environment, &args.config),
      ConfigSubCommand::Add(plugin_name_or_url) => commands::add_plugin_config_file(args, plugin_name_or_url.as_ref(), environment, plugin_resolver).await,
      ConfigSubCommand::Update { yes } => commands::update_plugins_config_file(args, environment, plugin_resolver, *yes).await,
    },
    SubCommand::Version => commands::output_version(environment),
    SubCommand::StdInFmt(cmd) => commands::stdin_fmt(cmd, args, environment, plugin_resolver, plugin_pools).await,
    SubCommand::OutputResolvedConfig => commands::output_resolved_config(args, environment, plugin_resolver).await,
    SubCommand::OutputFilePaths(cmd) => commands::output_file_paths(cmd, args, environment, plugin_resolver).await,
    SubCommand::OutputFormatTimes(cmd) => commands::output_format_times(cmd, args, environment, plugin_resolver, plugin_pools).await,
    SubCommand::Check(cmd) => commands::check(cmd, args, environment, plugin_resolver, plugin_pools).await,
    SubCommand::Fmt(cmd) => commands::format(cmd, args, environment, plugin_resolver, plugin_pools).await,
    SubCommand::Upgrade => commands::upgrade(environment).await,
    #[cfg(target_os = "windows")]
    SubCommand::Hidden(hidden_command) => match hidden_command {
      crate::arg_parser::HiddenSubCommand::WindowsInstall(install_path) => commands::handle_windows_install(environment, install_path),
      crate::arg_parser::HiddenSubCommand::WindowsUninstall(install_path) => commands::handle_windows_uninstall(environment, install_path),
    },
  }
}
