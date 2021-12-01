use std::io::Read;
use std::io::Write;
use std::sync::Arc;

use dprint_cli_core::types::ErrBox;
use dprint_core::plugins::process::start_parent_process_checker_thread;
use dprint_core::plugins::process::StdIoMessenger;
use dprint_core::plugins::process::StdIoReaderWriter;

use crate::arg_parser::CliArgs;
use crate::arg_parser::EditorServiceSubCommand;
use crate::cache::Cache;
use crate::configuration::resolve_config_from_args;
use crate::configuration::ResolvedConfig;
use crate::environment::Environment;
use crate::format::format_with_plugin_pools;
use crate::patterns::get_plugin_association_glob_matchers;
use crate::patterns::FileMatcher;
use crate::plugins::get_plugins_from_args;
use crate::plugins::resolve_plugins;
use crate::plugins::PluginPools;
use crate::plugins::PluginResolver;

pub fn output_editor_info<TEnvironment: Environment>(
  args: &CliArgs,
  cache: &Cache<TEnvironment>,
  environment: &TEnvironment,
  plugin_resolver: &PluginResolver<TEnvironment>,
) -> Result<(), ErrBox> {
  #[derive(serde::Serialize)]
  #[serde(rename_all = "camelCase")]
  struct EditorInfo {
    schema_version: u32,
    cli_version: String,
    config_schema_url: String,
    plugins: Vec<EditorPluginInfo>,
  }

  #[derive(serde::Serialize)]
  #[serde(rename_all = "camelCase")]
  struct EditorPluginInfo {
    name: String,
    version: String,
    config_key: String,
    file_extensions: Vec<String>,
    file_names: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    config_schema_url: Option<String>,
    help_url: String,
  }

  let mut plugins = Vec::new();

  for plugin in get_plugins_from_args(args, cache, environment, plugin_resolver)? {
    plugins.push(EditorPluginInfo {
      name: plugin.name().to_string(),
      version: plugin.version().to_string(),
      config_key: plugin.config_key().to_string(),
      file_extensions: plugin.file_extensions().iter().map(|ext| ext.to_string()).collect(),
      file_names: plugin.file_names().iter().map(|ext| ext.to_string()).collect(),
      config_schema_url: if plugin.config_schema_url().trim().is_empty() {
        None
      } else {
        Some(plugin.config_schema_url().trim().to_string())
      },
      help_url: plugin.help_url().to_string(),
    });
  }

  environment.log_silent(&serde_json::to_string(&EditorInfo {
    schema_version: 4,
    cli_version: env!("CARGO_PKG_VERSION").to_string(),
    config_schema_url: "https://dprint.dev/schemas/v0.json".to_string(),
    plugins,
  })?);

  Ok(())
}

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
          .log_stderr(&format!("Error canonicalizing file {}: {}", file_path.display(), err.to_string()));
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
      let association_matchers = get_plugin_association_glob_matchers(&plugins, &config.base_path)?;
      self.plugin_pools.set_plugins(plugins, association_matchers);
    }

    self.config = Some(config);

    Ok(())
  }
}

#[cfg(test)]
mod test {
  use dprint_core::plugins::process::StdIoMessenger;
  use dprint_core::plugins::process::StdIoReaderWriter;
  use dprint_core::types::ErrBox;
  use pretty_assertions::assert_eq;
  use std::io::Read;
  use std::io::Write;
  use std::path::Path;
  use std::path::PathBuf;

  use crate::environment::Environment;
  use crate::environment::TestEnvironmentBuilder;
  use crate::test_helpers::run_test_cli;

  #[test]
  fn it_should_output_editor_plugin_info() {
    // it should not output anything when downloading plugins
    let environment = TestEnvironmentBuilder::new()
      .add_remote_process_plugin()
      .add_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin().add_remote_process_plugin();
      })
      .build(); // build only, don't initialize
    run_test_cli(vec!["editor-info"], &environment).unwrap();
    let mut final_output = r#"{"schemaVersion":4,"cliVersion":""#.to_string();
    final_output.push_str(&env!("CARGO_PKG_VERSION").to_string());
    final_output.push_str(r#"","configSchemaUrl":"https://dprint.dev/schemas/v0.json","plugins":["#);
    final_output
      .push_str(r#"{"name":"test-plugin","version":"0.1.0","configKey":"test-plugin","fileExtensions":["txt"],"fileNames":[],"configSchemaUrl":"https://plugins.dprint.dev/schemas/test.json","helpUrl":"https://dprint.dev/plugins/test"},"#);
    final_output.push_str(r#"{"name":"test-process-plugin","version":"0.1.0","configKey":"testProcessPlugin","fileExtensions":["txt_ps"],"fileNames":["test-process-plugin-exact-file"],"helpUrl":"https://dprint.dev/plugins/test-process"}]}"#);
    assert_eq!(environment.take_stdout_messages(), vec![final_output]);
    assert_eq!(
      environment.take_stderr_messages(),
      vec![
        "Compiling https://plugins.dprint.dev/test-plugin.wasm",
        "Extracting zip for test-process-plugin"
      ]
    );
  }

  struct EditorServiceCommunicator {
    messenger: StdIoMessenger<Box<dyn Read + Send>, Box<dyn Write + Send>>,
  }

  impl EditorServiceCommunicator {
    pub fn new(stdin: Box<dyn Write + Send>, stdout: Box<dyn Read + Send>) -> Self {
      let reader_writer = StdIoReaderWriter::new(stdout, stdin);
      let messenger = StdIoMessenger::new(reader_writer);
      EditorServiceCommunicator { messenger }
    }

    pub fn check_file(&mut self, file_path: &Path) -> Result<bool, ErrBox> {
      self.messenger.send_message(1, vec![file_path.into()])?;
      let response_code = self.messenger.read_code()?;
      self.messenger.read_zero_part_message()?;
      Ok(response_code == 1)
    }

    pub fn format_text(&mut self, file_path: &Path, file_text: &str) -> Result<Option<String>, ErrBox> {
      self.messenger.send_message(2, vec![file_path.into(), file_text.into()])?;
      let response_code = self.messenger.read_code()?;
      match response_code {
        0 => {
          self.messenger.read_zero_part_message()?;
          Ok(None)
        }
        1 => Ok(Some(self.messenger.read_single_part_string_message()?)),
        2 => err!("{}", self.messenger.read_single_part_error_message()?),
        _ => err!("Unknown result: {}", response_code),
      }
    }

    pub fn exit(&mut self) {
      self.messenger.send_message(0, vec![]).unwrap();
    }
  }

  #[test]
  fn it_should_format_for_editor_service() {
    let txt_file_path = PathBuf::from("/file.txt");
    let ts_file_path = PathBuf::from("/file.ts");
    let other_ext_path = PathBuf::from("/file.asdf");
    let ignored_file_path = PathBuf::from("/ignored_file.txt");
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .add_remote_process_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin()
          .add_remote_process_plugin()
          .add_includes("**/*.{txt,ts}")
          .add_excludes("ignored_file.txt");
      })
      .write_file(&txt_file_path, "")
      .write_file(&ts_file_path, "")
      .write_file(&other_ext_path, "")
      .write_file(&ignored_file_path, "text")
      .initialize()
      .build();
    let stdin = environment.stdin_writer();
    let stdout = environment.stdout_reader();

    let result = std::thread::spawn({
      let environment = environment.clone();
      move || {
        let mut communicator = EditorServiceCommunicator::new(stdin, stdout);

        assert_eq!(communicator.check_file(&txt_file_path).unwrap(), true);
        assert_eq!(communicator.check_file(&PathBuf::from("/non-existent.txt")).unwrap(), true);
        assert_eq!(communicator.check_file(&other_ext_path).unwrap(), false);
        assert_eq!(communicator.check_file(&ts_file_path).unwrap(), true);
        assert_eq!(communicator.check_file(&ignored_file_path).unwrap(), false);

        assert_eq!(communicator.format_text(&txt_file_path, "testing").unwrap().unwrap(), "testing_formatted");
        assert_eq!(communicator.format_text(&txt_file_path, "testing_formatted").unwrap().is_none(), true); // it is already formatted
        assert_eq!(communicator.format_text(&other_ext_path, "testing").unwrap().is_none(), true); // can't format
        assert_eq!(
          communicator.format_text(&txt_file_path, "plugin: format this text").unwrap().unwrap(),
          "format this text_formatted_process"
        );
        assert_eq!(
          communicator.format_text(&txt_file_path, "should_error").err().unwrap().to_string(),
          "Did error."
        );
        assert_eq!(
          communicator.format_text(&txt_file_path, "plugin: should_error").err().unwrap().to_string(),
          "Did error."
        );
        assert_eq!(
          communicator.format_text(&PathBuf::from("/file.txt_ps"), "testing").unwrap().unwrap(),
          "testing_formatted_process"
        );

        // write a new file and make sure the service picks up the changes
        environment
          .write_file(
            &PathBuf::from("./dprint.json"),
            r#"{
                    "includes": ["**/*.txt"],
                    "test-plugin": {
                        "ending": "new_ending"
                    },
                    "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
                }"#,
          )
          .unwrap();

        assert_eq!(communicator.check_file(&ts_file_path).unwrap(), false); // shouldn't match anymore
        assert_eq!(communicator.check_file(&txt_file_path).unwrap(), true); // still ok
        assert_eq!(communicator.format_text(&txt_file_path, "testing").unwrap().unwrap(), "testing_new_ending");

        communicator.exit();
      }
    });

    // usually this would be the editor's process id, but this is ok for testing purposes
    let pid = std::process::id().to_string();
    run_test_cli(vec!["editor-service", "--parent-pid", &pid], &environment).unwrap();

    result.join().unwrap();
  }
}
