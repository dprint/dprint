use std::io::Read;
use std::io::Write;
use std::sync::Arc;

use dprint_cli_core::types::ErrBox;
use dprint_core::plugins::process::{start_parent_process_checker_thread, StdIoMessenger, StdIoReaderWriter};

use super::configuration::resolve_config_from_args;
use super::configuration::ResolvedConfig;
use super::format::format_with_plugin_pools;
use super::patterns::FileMatcher;
use super::plugins::resolve_plugins;
use super::{CliArgs, EditorServiceSubCommand};
use crate::cache::Cache;
use crate::environment::Environment;
use crate::plugins::{PluginPools, PluginResolver};

pub fn run_editor_service<TEnvironment: Environment>(
  args: &CliArgs,
  cache: &Cache<TEnvironment>,
  environment: &TEnvironment,
  plugin_resolver: &PluginResolver<TEnvironment>,
  plugin_pools: Arc<PluginPools<TEnvironment>>,
  editor_service_cmd: &EditorServiceSubCommand,
) -> Result<(), ErrBox> {
  // poll for the existence of the parent process and terminate this process when that process no longer exists
  let _handle = start_parent_process_checker_thread(editor_service_cmd.parent_pid);

  let mut editor_service = EditorService::new(args, cache, environment, plugin_resolver, plugin_pools);
  editor_service.run()
}

struct EditorService<'a, TEnvironment: Environment> {
  messenger: StdIoMessenger<Box<dyn Read + Send>, Box<dyn Write + Send>>,
  config: Option<ResolvedConfig>,
  args: &'a CliArgs,
  cache: &'a Cache<TEnvironment>,
  environment: &'a TEnvironment,
  plugin_resolver: &'a PluginResolver<TEnvironment>,
  plugin_pools: Arc<PluginPools<TEnvironment>>,
}

impl<'a, TEnvironment: Environment> EditorService<'a, TEnvironment> {
  pub fn new(
    args: &'a CliArgs,
    cache: &'a Cache<TEnvironment>,
    environment: &'a TEnvironment,
    plugin_resolver: &'a PluginResolver<TEnvironment>,
    plugin_pools: Arc<PluginPools<TEnvironment>>,
  ) -> Self {
    let stdin = environment.stdin();
    let stdout = environment.stdout();
    let reader_writer = StdIoReaderWriter::new(stdin, stdout);

    Self {
      messenger: StdIoMessenger::new(reader_writer),
      config: None,
      args,
      cache,
      environment,
      plugin_resolver,
      plugin_pools,
    }
  }

  pub fn run(&mut self) -> Result<(), ErrBox> {
    loop {
      let message_kind = self.messenger.read_code()?;
      match message_kind {
        // shutdown
        0 => return Ok(()),
        // check path
        1 => self.handle_check_path_message()?,
        // format
        2 => self.handle_format_message()?,
        // unknown, exit
        _ => return err!("Unknown message kind: {}", message_kind),
      }
    }
  }

  fn handle_check_path_message(&mut self) -> Result<(), ErrBox> {
    let file_path = self.messenger.read_single_part_path_buf_message()?;
    self.ensure_latest_config()?;

    let file_matcher = FileMatcher::new(&self.config.as_ref().unwrap(), self.args, self.environment)?;

    // canonicalize the file path, then check if it's in the list of file paths.
    match self.environment.canonicalize(&file_path) {
      Ok(resolved_file_path) => {
        log_verbose!(self.environment, "Checking can format: {}", resolved_file_path.display());
        self
          .messenger
          .send_message(if file_matcher.matches(&resolved_file_path) { 1 } else { 0 }, Vec::new())?;
      }
      Err(err) => {
        self
          .environment
          .log_error(&format!("Error canonicalizing file {}: {}", file_path.display(), err.to_string()));
        self.messenger.send_message(0, Vec::new())?; // don't format, something went wrong
      }
    }

    Ok(())
  }

  fn handle_format_message(&mut self) -> Result<(), ErrBox> {
    let mut parts = self.messenger.read_multi_part_message(2)?;
    let file_path = parts.take_path_buf()?;
    let file_text = parts.take_string()?;

    if self.config.is_none() {
      self.ensure_latest_config()?;
    }

    let formatted_text = format_with_plugin_pools(&file_path, &file_text, self.environment, &self.plugin_pools);
    match formatted_text {
      Ok(formatted_text) => {
        if formatted_text == file_text {
          self.messenger.send_message(0, Vec::new())?; // no change
        } else {
          self.messenger.send_message(
            1,
            vec![
              // change
              formatted_text.into(),
            ],
          )?;
        }
      }
      Err(err) => {
        self.messenger.send_message(
          2,
          vec![
            // error
            err.to_string().into(),
          ],
        )?;
      }
    }

    Ok(())
  }

  fn ensure_latest_config(&mut self) -> Result<(), ErrBox> {
    let last_config = self.config.take();
    let config = resolve_config_from_args(self.args, self.cache, self.environment)?;

    let has_config_changed = last_config.is_none() || last_config.unwrap() != config;
    if has_config_changed {
      self.plugin_pools.drop_plugins(); // clear the existing plugins
      let plugins = resolve_plugins(self.args, &config, self.environment, self.plugin_resolver)?;
      self.plugin_pools.set_plugins(plugins);
    }

    self.config = Some(config);

    Ok(())
  }
}
