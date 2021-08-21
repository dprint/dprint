use std::{borrow::Cow, path::Path, sync::Arc};

use dprint_cli_core::types::ErrBox;

use crate::cache::Cache;
use super::patterns::build_include_exclude_glob_sets;
use super::plugins::resolve_plugins;
use crate::environment::Environment;
use crate::plugins::{PluginPools, PluginResolver};
use super::{CliArgs, EditorServiceSubCommand};
use super::configuration::ResolvedConfig;
use super::configuration::resolve_config_from_args;
use super::format::format_with_plugin_pools;

pub fn run_editor_service<TEnvironment: Environment>(
  args: &CliArgs,
  cache: &Cache<TEnvironment>,
  environment: &TEnvironment,
  plugin_resolver: &PluginResolver<TEnvironment>,
  plugin_pools: Arc<PluginPools<TEnvironment>>,
  editor_service_cmd: &EditorServiceSubCommand,
) -> Result<(), ErrBox> {
  use dprint_core::plugins::process::{StdIoReaderWriter, StdIoMessenger, start_parent_process_checker_thread};

  // poll for the existence of the parent process and terminate this process when that process no longer exists
  let _handle = start_parent_process_checker_thread(editor_service_cmd.parent_pid);

  let stdin = environment.stdin();
  let stdout = environment.stdout();
  let reader_writer = StdIoReaderWriter::new(stdin, stdout);
  let mut messenger = StdIoMessenger::new(reader_writer);
  let mut past_config: Option<ResolvedConfig> = None;

  loop {
      let message_kind = messenger.read_code()?;
      match message_kind {
          // shutdown
          0 => return Ok(()),
          // check path
          1 => {
              let file_path = messenger.read_single_part_path_buf_message()?;
              // update the glob file patterns and absolute paths by re-retrieving the config file
              let config = resolve_config_from_args(args, cache, environment)?;
              let (include_globset, exclude_globset) = build_include_exclude_glob_sets(&config, args, environment)?;

              // canonicalize the file path, then check if it's in the list of file paths.
              match environment.canonicalize(&file_path) {
                  Ok(resolved_file_path) => {
                      let matches_includes = include_globset.is_match(&resolved_file_path);
                      let matches_excludes = exclude_globset.is_match(&resolved_file_path);
                      messenger.send_message(if matches_includes && !matches_excludes { 1 } else { 0 }, Vec::new())?;
                  },
                  Err(err) => {
                      environment.log_error(&format!("Error canonicalizing file {}: {}", file_path.display(), err.to_string()));
                      messenger.send_message(0, Vec::new())?; // don't format, something went wrong
                  }
              }
          },
          // format
          2 => {
              let mut parts = messenger.read_multi_part_message(2)?;
              let file_path = parts.take_path_buf()?;
              let file_text = parts.take_string()?;

              let result = format_text(args, cache, environment, plugin_resolver, &plugin_pools, &past_config, &file_path, &file_text);
              match result {
                  Ok((formatted_text, config)) => {
                      if formatted_text == file_text {
                          messenger.send_message(0, Vec::new())?; // no change
                      } else {
                          messenger.send_message(1, vec![ // change
                              formatted_text.into()
                          ])?;
                      }

                      past_config.replace(config);
                  },
                  Err(err) => {
                      messenger.send_message(2, vec![ // error
                          err.to_string().into()
                      ])?;
                  }
              }
          },
          _ => {
              environment.log_error(&format!("Unknown message kind: {}", message_kind));
          }
      }
  }

  fn format_text<'a, TEnvironment: Environment>(
      args: &CliArgs,
      cache: &Cache<TEnvironment>,
      environment: &TEnvironment,
      plugin_resolver: &PluginResolver<TEnvironment>,
      plugin_pools: &Arc<PluginPools<TEnvironment>>,
      past_config: &Option<ResolvedConfig>,
      file_path: &Path,
      file_text: &'a str,
  ) -> Result<(Cow<'a, str>, ResolvedConfig), ErrBox> {
      let config = resolve_config_from_args(&args, cache, environment)?;
      let has_config_changed = past_config.is_none() || *past_config.as_ref().unwrap() != config;
      if has_config_changed {
          plugin_pools.drop_plugins(); // clear the existing plugins
          let plugins = resolve_plugins(&config, environment, plugin_resolver)?;
          plugin_pools.set_plugins(plugins);
      }

      let formatted_text = format_with_plugin_pools(&file_path, &file_text, environment, &plugin_pools)?;
      Ok((formatted_text, config))
  }
}
