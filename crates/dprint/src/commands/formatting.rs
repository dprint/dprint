use anyhow::Result;
use crossterm::style::Stylize;
use dprint_core::plugins::HostFormatRequest;
use dprint_core::plugins::NullCancellationToken;
use parking_lot::Mutex;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use thiserror::Error;

use crate::arg_parser::CheckSubCommand;
use crate::arg_parser::CliArgs;
use crate::arg_parser::FmtSubCommand;
use crate::arg_parser::OutputFormatTimesSubCommand;
use crate::arg_parser::StdInFmtSubCommand;
use crate::configuration::resolve_config_from_args;
use crate::environment::Environment;
use crate::format::run_parallelized;
use crate::format::EnsureStableFormat;
use crate::incremental::get_incremental_file;
use crate::patterns::FileMatcher;
use crate::plugins::PluginResolver;
use crate::resolution::resolve_plugins_scope;
use crate::resolution::resolve_plugins_scope_and_paths;
use crate::resolution::PluginsScope;
use crate::utils::get_difference;
use crate::utils::AtomicCounter;
use crate::utils::BOM_BYTES;

pub async fn stdin_fmt<TEnvironment: Environment>(
  cmd: &StdInFmtSubCommand,
  args: &CliArgs,
  environment: &TEnvironment,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
) -> Result<()> {
  let config = Rc::new(resolve_config_from_args(args, environment).await?);
  let plugins_scope = Rc::new(resolve_plugins_scope(config, environment, plugin_resolver).await?);
  plugins_scope.ensure_plugins_found()?;
  plugins_scope.ensure_no_global_config_diagnostics()?;

  // if the path is absolute, then apply exclusion rules
  if environment.is_absolute_path(&cmd.file_name_or_path) {
    let file_matcher = FileMatcher::new(plugins_scope.config.as_ref().unwrap(), &cmd.patterns, &environment.cwd())?;
    // canonicalize the file path, then check if it's in the list of file paths.
    let resolved_file_path = environment.canonicalize(&cmd.file_name_or_path)?;
    // log the file text as-is since it's not in the list of files to format
    if !file_matcher.matches(resolved_file_path) {
      environment.log_machine_readable(&cmd.file_bytes);
      return Ok(());
    }
  }
  output_stdin_format(PathBuf::from(&cmd.file_name_or_path), &cmd.file_bytes, plugins_scope, environment).await
}

async fn output_stdin_format<TEnvironment: Environment>(
  file_path: PathBuf,
  file_bytes: &[u8],
  plugins_scope: Rc<PluginsScope<TEnvironment>>,
  environment: &TEnvironment,
) -> Result<()> {
  let result = plugins_scope
    .format(HostFormatRequest {
      file_path,
      file_bytes: file_bytes.to_vec(),
      range: None,
      override_config: Default::default(),
      token: Arc::new(NullCancellationToken),
    })
    .await?;
  match result {
    Some(text) => environment.log_machine_readable(&text),
    None => environment.log_machine_readable(file_bytes),
  }
  Ok(())
}

pub async fn output_format_times<TEnvironment: Environment>(
  cmd: &OutputFormatTimesSubCommand,
  args: &CliArgs,
  environment: &TEnvironment,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
) -> Result<()> {
  let scopes = resolve_plugins_scope_and_paths(args, &cmd.patterns, environment, plugin_resolver).await?;
  scopes.ensure_valid_for_cli_args(args)?;
  let durations: Arc<Mutex<Vec<(PathBuf, u128)>>> = Arc::new(Mutex::new(Vec::new()));

  for scope_and_paths in scopes.into_iter() {
    run_parallelized(scope_and_paths, environment, None, EnsureStableFormat(false), {
      let durations = durations.clone();
      move |file_path, _, _, start_instant, _| {
        let duration = start_instant.elapsed().as_millis();
        durations.lock().push((file_path, duration));
        Ok(())
      }
    })
    .await?;
  }

  let mut durations = durations.lock();
  durations.sort_by_key(|k| k.1);
  for (file_path, duration) in durations.iter() {
    log_stdout_info!(environment, "{}ms - {}", duration, file_path.display());
  }

  Ok(())
}

#[derive(Error, Debug)]
#[error("{}", match files_count {
  Some(files_count) => format!(
    "Found {} not formatted {}.",
    files_count.to_string().bold(),
    if *files_count == 1 { "file" } else { "files" },
  ),
  None => "".to_string(), // no output for list-different
})]
pub struct CheckError {
  pub files_count: Option<usize>,
}

pub async fn check<TEnvironment: Environment>(
  cmd: &CheckSubCommand,
  args: &CliArgs,
  environment: &TEnvironment,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
) -> Result<()> {
  let scopes = resolve_plugins_scope_and_paths(args, &cmd.patterns, environment, plugin_resolver).await?;
  scopes.ensure_valid_for_cli_args(args)?;
  let not_formatted_files_count = Arc::new(AtomicCounter::default());
  let list_different = cmd.list_different;

  for scope_and_paths in scopes.into_iter() {
    let incremental_file = scope_and_paths
      .scope
      .config
      .as_ref()
      .and_then(|config| get_incremental_file(cmd.incremental, config, &scope_and_paths.scope, environment))
      .map(Arc::new);
    run_parallelized(scope_and_paths, environment, incremental_file.clone(), EnsureStableFormat(false), {
      let not_formatted_files_count = not_formatted_files_count.clone();
      let incremental_file = incremental_file.clone();
      move |file_path, file_bytes, formatted_bytes, _, environment| {
        if formatted_bytes != file_bytes.as_ref() {
          not_formatted_files_count.inc();
          if list_different {
            log_stdout_info!(environment, "{}", file_path.display());
          } else {
            output_difference(&file_path, file_bytes.as_ref(), &formatted_bytes, &environment);
          }
        } else {
          // update the incremental cache when the file is already formatted correctly
          // so that this runs faster next time, but don't update it with the
          // correctly formatted file because it hasn't undergone a stable
          // formatting check
          if let Some(incremental_file) = &incremental_file {
            incremental_file.update_file(&formatted_bytes);
          }
        }
        Ok(())
      }
    })
    .await?;

    if let Some(incremental_file) = &incremental_file {
      incremental_file.write();
    }
  }

  let not_formatted_files_count = not_formatted_files_count.get();
  if not_formatted_files_count == 0 {
    Ok(())
  } else {
    Err(
      CheckError {
        files_count: if list_different { None } else { Some(not_formatted_files_count) },
      }
      .into(),
    )
  }
}

fn output_difference(file_path: &Path, file_bytes: &[u8], formatted_bytes: &[u8], environment: &impl Environment) {
  let file_text = match String::from_utf8(file_bytes.to_vec()) {
    Ok(text) => text,
    Err(err) => {
      log_warn!(
        environment,
        "Failed outputting difference for {}. Could not get original text as utf-8. {:#}",
        file_path.display(),
        err
      );
      return;
    }
  };
  let formatted_text = match String::from_utf8(formatted_bytes.to_vec()) {
    Ok(text) => text,
    Err(err) => {
      log_warn!(
        environment,
        "Failed outputting difference for {}. Coult not get formatted text as utf-8. {:#}",
        file_path.display(),
        err
      );
      return;
    }
  };
  let difference_text = get_difference(&file_text, &formatted_text);
  log_stdout_info!(environment, "{} {}:\n{}\n--", "from".bold().red(), file_path.display(), difference_text);
}

pub async fn format<TEnvironment: Environment>(
  cmd: &FmtSubCommand,
  args: &CliArgs,
  environment: &TEnvironment,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
) -> Result<()> {
  let scopes = resolve_plugins_scope_and_paths(args, &cmd.patterns, environment, plugin_resolver).await?;
  scopes.ensure_valid_for_cli_args(args)?;

  let formatted_files_count = Arc::new(AtomicCounter::default());
  for scope_and_paths in scopes.into_iter() {
    let incremental_file = scope_and_paths
      .scope
      .config
      .as_ref()
      .and_then(|config| get_incremental_file(cmd.incremental, config, &scope_and_paths.scope, environment))
      .map(Arc::new);
    let output_diff = cmd.diff;

    run_parallelized(
      scope_and_paths,
      environment,
      incremental_file.clone(),
      EnsureStableFormat(cmd.enable_stable_format),
      {
        let formatted_files_count = formatted_files_count.clone();
        let incremental_file = incremental_file.clone();
        move |file_path, file_bytes, formatted_bytes, _, environment| {
          if let Some(incremental_file) = &incremental_file {
            incremental_file.update_file(&formatted_bytes);
          }

          if formatted_bytes != file_bytes.as_ref() {
            if output_diff {
              output_difference(&file_path, file_bytes.as_ref(), &formatted_bytes, &environment);
            }

            let new_text = if file_bytes.has_bom() {
              // add back the BOM
              let mut new_bytes = Vec::with_capacity(file_bytes.as_ref().len() + BOM_BYTES.len());
              new_bytes.extend_from_slice(BOM_BYTES);
              new_bytes.extend(formatted_bytes);
              new_bytes
            } else {
              formatted_bytes
            };

            formatted_files_count.inc();
            environment.write_file_bytes(file_path, &new_text)?;
          }

          Ok(())
        }
      },
    )
    .await?;

    if let Some(incremental_file) = &incremental_file {
      incremental_file.write();
    }
  }

  let formatted_files_count = formatted_files_count.get();
  if formatted_files_count > 0 {
    let suffix = if formatted_files_count == 1 { "file" } else { "files" };
    log_stdout_info!(environment, "Formatted {} {}.", formatted_files_count.to_string().bold(), suffix);
  }

  Ok(())
}

#[cfg(test)]
mod test {
  use crossterm::style::Stylize;
  use pretty_assertions::assert_eq;

  use crate::environment::Environment;
  use crate::environment::TestEnvironment;
  use crate::environment::TestEnvironmentBuilder;
  use crate::test_helpers;
  use crate::test_helpers::get_plural_check_text;
  use crate::test_helpers::get_plural_formatted_text;
  use crate::test_helpers::get_singular_check_text;
  use crate::test_helpers::get_singular_formatted_text;
  use crate::test_helpers::run_test_cli;
  use crate::test_helpers::run_test_cli_with_stdin;
  use crate::test_helpers::TestAppError;
  use crate::test_helpers::TestProcessPluginFile;
  use crate::test_helpers::TestProcessPluginFileBuilder;
  use crate::test_helpers::PROCESS_PLUGIN_ZIP_CHECKSUM;
  use crate::utils::get_difference;
  use crate::utils::TestStdInReader;

  #[test]
  fn should_output_format_times() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin()
      .write_file("/file.txt", "const t=4;")
      .write_file("/file2.txt", "const t=4;")
      .write_file("/file3.txt_ps", "const t=4;")
      .build();
    run_test_cli(vec!["output-format-times", "**/*.*"], &environment).unwrap();
    let logged_messages = environment.take_stdout_messages();
    assert_eq!(logged_messages.len(), 3); // good enough
  }

  #[test]
  fn should_format_single_file() {
    let file_path1 = "/file.txt";
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
      .write_file(file_path1, "text")
      .build();
    run_test_cli(vec!["fmt", "/file.txt"], &environment).unwrap();
    assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);
    assert_eq!(environment.read_file(&file_path1).unwrap(), "text_formatted");
  }

  #[test]
  fn should_format_files() {
    let file_path1 = "/file.txt";
    let file_path2 = "/file.txt_ps";
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin()
      .write_file(&file_path1, "text")
      .write_file(&file_path2, "text2")
      .build();
    environment.set_max_threads(1); // ensure files are still formatted with only 1 core
    run_test_cli(vec!["fmt", "/file.*"], &environment).unwrap();
    assert_eq!(environment.take_stdout_messages(), vec![get_plural_formatted_text(2)]);
    assert_eq!(environment.read_file(&file_path1).unwrap(), "text_formatted");
    assert_eq!(environment.read_file(&file_path2).unwrap(), "text2_formatted_process");
  }

  #[test]
  fn should_format_plugin_explicitly_specified_files() {
    // this file name is mentioned in test-process-plugin's PluginInfo
    let file_path1 = "/test-process-plugin-exact-file";
    let environment = TestEnvironmentBuilder::with_initialized_remote_process_plugin()
      .write_file(&file_path1, "text")
      .build();
    run_test_cli(vec!["fmt", "*"], &environment).unwrap();
    assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);
    assert_eq!(environment.read_file(&file_path1).unwrap(), "text_formatted_process");
  }

  #[test]
  fn should_format_files_with_local_plugin() {
    let file_path = "/file.txt";
    let environment = TestEnvironmentBuilder::new()
      .add_local_wasm_plugin()
      .with_default_config(|c| {
        c.add_local_wasm_plugin();
      })
      .write_file(&file_path, "text")
      .initialize()
      .build();
    run_test_cli(vec!["fmt", "/file.txt"], &environment).unwrap();
    assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);
    assert_eq!(environment.read_file(&file_path).unwrap(), "text_formatted");
  }

  #[test]
  fn should_handle_wasm_plugin_erroring() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
      .write_file("/file.txt", "should_error") // special text that makes the plugin error
      .build();
    let error_message = run_test_cli(vec!["fmt", "/file.txt"], &environment).err().unwrap();
    assert_eq!(
      environment.take_stderr_messages(),
      vec![String::from("Error formatting /file.txt. Message: Did error.")]
    );
    assert_eq!(error_message.to_string(), "Had 1 error(s) formatting.");
  }

  #[test]
  fn should_handle_process_plugin_erroring() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_process_plugin()
      .write_file("/file.txt_ps", "should_error") // special text that makes the plugin error
      .build();
    let error_message = run_test_cli(vec!["fmt", "/file.txt_ps"], &environment).err().unwrap();
    assert_eq!(
      environment.take_stderr_messages(),
      vec![String::from("Error formatting /file.txt_ps. Message: Did error.")]
    );
    assert_eq!(error_message.to_string(), "Had 1 error(s) formatting.");
  }

  #[test]
  fn should_handle_wasm_plugin_panicking() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin()
      .write_file("/file1.txt", "should_panic") // special text to make it panic
      .write_file("/file2.txt_ps", "test")
      .build();
    let error_message = run_test_cli(vec!["fmt", "**.{txt,txt_ps}"], &environment).err().unwrap();
    let logged_errors = environment.take_stderr_messages();
    assert_eq!(logged_errors.len(), 1);
    let expected_start_text = concat!(
      "Critical error formatting /file1.txt. Cannot continue. ",
      "Message: Originally panicked in test-plugin, then failed reinitialize. ",
      "This may be a bug in the plugin, the dprint cli is out of date, or the plugin is out of date.",
    );
    assert_eq!(&logged_errors[0][..expected_start_text.len()], expected_start_text);
    assert_eq!(error_message.to_string(), "Had 1 error(s) formatting.");
    // should still format with the other plugin
    assert_eq!(environment.read_file("/file2.txt_ps").unwrap(), "test_formatted_process");
  }

  #[test]
  fn should_format_calling_process_plugin_with_wasm_plugin_and_no_plugin_exists() {
    let file_path = "/file.txt";
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
      .write_file(&file_path, "plugin: format this text")
      .build();
    run_test_cli(vec!["fmt", "/file.txt"], &environment).unwrap();
    assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);
    assert_eq!(environment.read_file(&file_path).unwrap(), "plugin: format this text_formatted");
  }

  #[test]
  fn should_format_calling_process_plugin_with_wasm_plugin_and_process_plugin_exists() {
    let file_path = "/file.txt";
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin()
      .write_file(&file_path, "plugin: format this text")
      .build();
    run_test_cli(vec!["fmt", "/file.txt"], &environment).unwrap();
    assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);
    assert_eq!(
      environment.read_file(&file_path).unwrap(),
      "plugin: format this text_formatted_process_formatted"
    );
  }

  #[test]
  fn should_format_calling_process_plugin_with_wasm_plugin_using_additional_plugin_specified_config() {
    let file_path1 = "/file1.txt";
    let file_path2 = "/file2.txt";
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin()
      .write_file(&file_path1, "plugin-config: format this text")
      .write_file(&file_path2, "plugin: format this text")
      .build();
    run_test_cli(vec!["fmt", "/*.txt"], &environment).unwrap();
    assert_eq!(environment.take_stdout_messages(), vec![get_plural_formatted_text(2)]);
    assert_eq!(
      environment.read_file(&file_path1).unwrap(),
      "plugin-config: format this text_custom_config_formatted"
    );
    assert_eq!(
      environment.read_file(&file_path2).unwrap(),
      "plugin: format this text_formatted_process_formatted"
    );
  }

  #[test]
  fn should_error_calling_process_plugin_with_wasm_plugin_and_process_plugin_errors() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin()
      .write_file("/file.txt", "plugin: should_error")
      .build();
    let error_message = run_test_cli(vec!["fmt", "/file.txt"], &environment).err().unwrap();
    assert_eq!(error_message.to_string(), "Had 1 error(s) formatting.");
    assert_eq!(
      environment.take_stderr_messages(),
      vec![String::from("Error formatting /file.txt. Message: Did error.")]
    );
  }

  #[test]
  fn should_format_calling_other_plugin_with_process_plugin_and_no_plugin_exists() {
    let file_path = "/file.txt_ps";
    let environment = TestEnvironmentBuilder::with_initialized_remote_process_plugin()
      .write_file(&file_path, "plugin: format this text")
      .build();
    run_test_cli(vec!["fmt", "/file.txt_ps"], &environment).unwrap();
    assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);
    assert_eq!(environment.read_file(&file_path).unwrap(), "plugin: format this text_formatted_process");
  }

  #[test]
  fn should_format_calling_wasm_plugin_with_process_plugin_and_wasm_plugin_exists() {
    let file_path = "/file.txt_ps";
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin()
      .write_file(&file_path, "plugin: format this text")
      .build();
    run_test_cli(vec!["fmt", "/file.txt_ps"], &environment).unwrap();
    assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);
    assert_eq!(
      environment.read_file(&file_path).unwrap(),
      "plugin: format this text_formatted_formatted_process"
    );
  }

  #[test]
  fn should_format_calling_wasm_plugin_with_process_plugin_using_additional_plugin_specified_config() {
    let file_path1 = "/file1.txt_ps";
    let file_path2 = "/file2.txt_ps";
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin()
      .write_file(&file_path1, "plugin-config: format this text")
      .write_file(&file_path2, "plugin: format this text")
      .build();
    run_test_cli(vec!["fmt", "*.txt_ps"], &environment).unwrap();
    assert_eq!(environment.take_stdout_messages(), vec![get_plural_formatted_text(2)]);
    assert_eq!(
      environment.read_file(&file_path1).unwrap(),
      "plugin-config: format this text_custom_config_formatted_process"
    );
    assert_eq!(
      environment.read_file(&file_path2).unwrap(),
      "plugin: format this text_formatted_formatted_process"
    );
  }

  #[test]
  fn should_error_calling_wasm_plugin_with_process_plugin_and_wasm_plugin_errors() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin()
      .write_file("/file.txt_ps", "plugin: should_error")
      .build();
    let error_message = run_test_cli(vec!["fmt", "/file.txt_ps"], &environment).err().unwrap();
    assert_eq!(error_message.to_string(), "Had 1 error(s) formatting.");
    assert_eq!(
      environment.take_stderr_messages(),
      vec![String::from("Error formatting /file.txt_ps. Message: Did error.")]
    );
  }

  #[test]
  fn should_format_when_specifying_dot_slash_paths() {
    let file_path = "/file.txt";
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
      .write_file(&file_path, "text")
      .build();
    run_test_cli(vec!["fmt", "./file.txt"], &environment).unwrap();
    assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);
    assert_eq!(environment.read_file(&file_path).unwrap(), "text_formatted");
  }

  #[test]
  fn should_exclude_a_specified_dot_slash_path() {
    let file_path = "/file.txt";
    let file_path2 = "/file2.txt";
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
      .write_file(&file_path, "text")
      .write_file(&file_path2, "text")
      .build();
    run_test_cli(vec!["fmt", "./**/*.txt", "--excludes", "./file2.txt"], &environment).unwrap();
    assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);
    assert_eq!(environment.read_file(&file_path).unwrap(), "text_formatted");
    assert_eq!(environment.read_file(&file_path2).unwrap(), "text");
  }

  #[test]
  fn should_ignore_files_in_node_modules_by_default() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
      .write_file("/node_modules/file.txt", "")
      .write_file("/test/node_modules/file.txt", "")
      .write_file("/file.txt", "")
      .build();
    run_test_cli(vec!["fmt", "**/*.txt"], &environment).unwrap();
    assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);
  }

  #[test]
  fn should_not_ignore_files_in_node_modules_when_allowed() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
      .write_file("/node_modules/file.txt", "const t=4;")
      .write_file("/test/node_modules/file.txt", "const t=4;")
      .build();
    run_test_cli(vec!["fmt", "--allow-node-modules", "**/*.txt"], &environment).unwrap();
    assert_eq!(environment.take_stdout_messages(), vec![get_plural_formatted_text(2)]);
  }

  #[test]
  fn should_format_files_with_config() {
    let file_path1 = "/file1.txt";
    let file_path2 = "/file2.txt_ps";
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin()
      .with_local_config("/config.json", |c| {
        c.add_remote_wasm_plugin()
          .add_remote_process_plugin()
          .add_config_section(
            "test-plugin",
            r#"{
                        "ending": "custom-formatted"
                    }"#,
          )
          .add_config_section(
            "testProcessPlugin",
            r#"{
                        "ending": "custom-formatted2"
                    }"#,
          );
      })
      .write_file(&file_path1, "text")
      .write_file(&file_path2, "text2")
      .build();

    run_test_cli(vec!["fmt", "--config", "/config.json", "/file1.txt", "/file2.txt_ps"], &environment).unwrap();

    assert_eq!(environment.take_stdout_messages(), vec![get_plural_formatted_text(2)]);
    assert_eq!(environment.read_file(&file_path1).unwrap(), "text_custom-formatted");
    assert_eq!(environment.read_file(&file_path2).unwrap(), "text2_custom-formatted2");
  }

  #[test]
  fn should_format_files_with_config_sub_dir_auto_discoverable_name() {
    let file_path1 = "/file1.txt";
    let file_path2 = "/file2.txt_ps";
    let file_path3 = "/other_dir/file1.txt";
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin()
      .with_local_config("/sub_dir/dprint.json", |c| {
        c.add_remote_wasm_plugin()
          .add_remote_process_plugin()
          .add_config_section(
            "test-plugin",
            r#"{
                        "ending": "custom-formatted"
                    }"#,
          )
          .add_config_section(
            "testProcessPlugin",
            r#"{
                        "ending": "custom-formatted2"
                    }"#,
          )
          .add_excludes("./excludes");
      })
      .with_local_config("/other_dir/dprint.json", |c| {
        c.add_remote_wasm_plugin().add_config_section(
          "test-plugin",
          r#"{
              "ending": "other-ending"
            }"#,
        );
      })
      .write_file(&file_path1, "text")
      .write_file(&file_path2, "text2")
      .write_file(&file_path3, "text3")
      .write_file("./excludes/file1.txt", "text4")
      .build();

    run_test_cli(vec!["fmt", "--config", "/sub_dir/dprint.json"], &environment).unwrap();

    assert_eq!(environment.take_stdout_messages(), vec![get_plural_formatted_text(3)]);
    assert_eq!(environment.read_file(&file_path1).unwrap(), "text_custom-formatted");
    assert_eq!(environment.read_file(&file_path2).unwrap(), "text2_custom-formatted2");
    assert_eq!(environment.read_file(&file_path3).unwrap(), "text3_other-ending");
  }

  #[test]
  fn should_format_files_with_config_using_c() {
    let file_path1 = "/file1.txt";
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
      .write_file(file_path1, "text")
      .write_file(
        "/config.json",
        r#"{
                "test-plugin": { "ending": "custom-formatted" },
                "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
            }"#,
      )
      .build();

    run_test_cli(vec!["fmt", "-c", "/config.json", "/file1.txt"], &environment).unwrap();

    assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);
    assert_eq!(environment.read_file(&file_path1).unwrap(), "text_custom-formatted");
  }

  #[test]
  fn should_error_when_config_file_does_not_exist() {
    let environment = TestEnvironment::new();
    environment.write_file("/test.txt", "test").unwrap();

    let err = run_test_cli(vec!["fmt", "**/*.txt"], &environment).err().unwrap();
    err.assert_exit_code(11);

    assert_eq!(
      err.to_string(),
      concat!(
        "No config file found at /dprint.json. Did you mean to create (dprint init) or specify one (--config <path>)?\n",
        "  Error: Could not find file at path /dprint.json"
      )
    );
  }

  #[test]
  fn should_support_config_file_urls() {
    let file_path1 = "/file1.txt";
    let file_path2 = "/file2.txt";
    let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
      .with_remote_config("https://dprint.dev/test.json", |c| {
        c.add_remote_wasm_plugin()
          .add_config_section("test-plugin", r#"{ "ending": "custom-formatted" }"#);
      })
      .write_file(&file_path1, "text")
      .write_file(&file_path2, "text2")
      .build();

    run_test_cli(
      vec!["fmt", "--config", "https://dprint.dev/test.json", "/file1.txt", "/file2.txt"],
      &environment,
    )
    .unwrap();

    assert_eq!(environment.take_stdout_messages(), vec![get_plural_formatted_text(2)]);
    assert_eq!(
      environment.take_stderr_messages(),
      vec!["Compiling https://plugins.dprint.dev/test-plugin.wasm"]
    );
    assert_eq!(environment.read_file(&file_path1).unwrap(), "text_custom-formatted");
    assert_eq!(environment.read_file(&file_path2).unwrap(), "text2_custom-formatted");
  }

  #[test]
  fn should_format_files_with_config_associations() {
    let file_path1 = "/file1.txt";
    let file_path2 = "/file2.txt_ps";
    let file_path3 = "/file2.other";
    let file_path4 = "/src/some_file_name";
    let file_path5 = "/src/sub-dir/test-process-plugin-exact-file";
    let file_path6 = "/file6.txt_ps";
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin()
      .with_local_config("/config.json", |c| {
        c.add_remote_wasm_plugin()
          .add_remote_process_plugin()
          .add_config_section(
            "test-plugin",
            r#"{
              "associations": [
                "**/*.txt_ps",
                "test-process-plugin-exact-file"
              ],
              "ending": "wasm"
            }"#,
          )
          .add_config_section(
            "testProcessPlugin",
            r#"{
              "associations": [
                "**/*.other",
                "some_file_name",
              ]
              "ending": "ps"
            }"#,
          );
      })
      .write_file(&file_path1, "text")
      .write_file(&file_path2, "text2")
      .write_file(&file_path3, "text3")
      .write_file(&file_path4, "text4")
      .write_file(&file_path5, "text5")
      .write_file(&file_path6, "plugin: text6")
      .build();

    run_test_cli(vec!["fmt", "--config", "/config.json"], &environment).unwrap();

    assert_eq!(environment.take_stdout_messages(), vec![get_plural_formatted_text(5)]);
    assert_eq!(environment.read_file(&file_path1).unwrap(), "text"); // not matched in any associations
    assert_eq!(environment.read_file(&file_path2).unwrap(), "text2_wasm");
    assert_eq!(environment.read_file(&file_path3).unwrap(), "text3_ps");
    assert_eq!(environment.read_file(&file_path4).unwrap(), "text4_ps");
    assert_eq!(environment.read_file(&file_path5).unwrap(), "text5_wasm");
    // this will request formatting a .txt_ps file, but should be caught be the associations
    assert_eq!(environment.read_file(&file_path6).unwrap(), "plugin: text6_wasm_wasm");
  }

  #[test]
  fn should_format_files_with_config_associations_multiple_plugins_same_files() {
    let file_path1 = "/file1.txt";
    let file_path2 = "/file2.txt_ps";
    let file_path3 = "/file2.other";
    let file_path4 = "/src/some_file_name";
    let file_path5 = "/src/sub-dir/test-process-plugin-exact-file";
    let file_path6 = "/file6.txt";
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin()
      .with_local_config("/config.json", |c| {
        c.set_incremental(false)
          .add_remote_wasm_plugin()
          .add_remote_process_plugin()
          .add_config_section(
            "test-plugin",
            r#"{
              "associations": [
                "**/*.{txt,txt_ps,other}",
                "some_file_name",
                "test-process-plugin-exact-file"
              ],
              "ending": "wasm"
            }"#,
          )
          .add_config_section(
            "testProcessPlugin",
            r#"{
              "associations": [
                "**/*.{txt,txt_ps,other}",
                "some_file_name",
                "test-process-plugin-exact-file"
              ],
              "ending": "ps"
            }"#,
          );
      })
      .write_file(&file_path1, "text")
      .write_file(&file_path2, "text2")
      .write_file(&file_path3, "text3")
      .write_file(&file_path4, "text4")
      .write_file(&file_path5, "text5")
      .write_file(&file_path6, "plugin: text6")
      .build();

    run_test_cli(vec!["fmt", "--config", "/config.json", "--skip-stable-format"], &environment).unwrap();

    assert_eq!(environment.take_stdout_messages(), vec![get_plural_formatted_text(6)]);
    assert_eq!(environment.read_file(&file_path1).unwrap(), "text_wasm_ps");
    assert_eq!(environment.read_file(&file_path2).unwrap(), "text2_wasm_ps");
    assert_eq!(environment.read_file(&file_path3).unwrap(), "text3_wasm_ps");
    assert_eq!(environment.read_file(&file_path4).unwrap(), "text4_wasm_ps");
    assert_eq!(environment.read_file(&file_path5).unwrap(), "text5_wasm_ps");
    assert_eq!(environment.read_file(&file_path6).unwrap(), "plugin: text6_wasm_ps_wasm_ps_ps");
  }

  #[test]
  fn should_format_files_all_negated_associations_no_config_excludes() {
    let file_path1 = "/file1.txt";
    let file_path2 = "/file2.txt";
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin()
      .with_local_config("/config.json", |c| {
        c.add_remote_wasm_plugin().add_config_section(
          "test-plugin",
          r#"{
              "associations": [
                "!**/file2.txt",
                "!**/file3.txt",
              ],
              "ending": "wasm"
            }"#,
        );
      })
      .write_file(&file_path1, "text")
      .write_file(&file_path2, "text2")
      .build();

    run_test_cli(vec!["fmt", "--config", "/config.json"], &environment).unwrap();

    assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);
    assert_eq!(environment.read_file(&file_path1).unwrap(), "text_wasm");
    assert_eq!(environment.read_file(&file_path2).unwrap(), "text2"); // ignored
  }

  #[test]
  fn should_error_on_wasm_plugin_config_diagnostic() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_config_section("test-plugin", r#"{ "non-existent": 25 }"#);
      })
      .write_file("/test.txt", "test")
      .build();

    let error_message = run_test_cli(vec!["fmt", "**/*.txt"], &environment).err().unwrap();

    assert_eq!(error_message.to_string(), "Had 1 error(s) formatting.");
    assert_eq!(
      environment.take_stderr_messages(),
      vec![
        "[test-plugin]: Unknown property in configuration (non-existent)",
        "[test-plugin]: Error initializing from configuration file. Had 1 diagnostic(s)."
      ]
    );
  }

  #[test]
  fn should_error_on_process_plugin_config_diagnostic() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_process_plugin()
      .with_default_config(|c| {
        // Add this same plugin a few times in the configuration file for
        // some additional testing. It should handle it gracefully.
        c.add_remote_process_plugin();
        c.add_remote_process_plugin();

        c.add_config_section(
          "testProcessPlugin",
          r#"{
                    "non-existent": 25
                }"#,
        );
      })
      .write_file("/test.txt_ps", "test")
      .build();

    let error_message = run_test_cli(vec!["fmt", "**/*.txt_ps"], &environment).err().unwrap();

    assert_eq!(error_message.to_string(), "Had 1 error(s) formatting.");
    assert_eq!(
      environment.take_stderr_messages(),
      vec![
        "[test-process-plugin]: Unknown property in configuration (non-existent)",
        "[test-process-plugin]: Error initializing from configuration file. Had 1 diagnostic(s)."
      ]
    );
  }

  #[test]
  fn should_error_config_diagnostic_multiple_plugins_same_file_via_associations() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin()
      .with_local_config("/config.json", |c| {
        c.add_remote_wasm_plugin()
          .add_remote_process_plugin()
          .add_config_section(
            "test-plugin",
            r#"{
              "associations": [
                "shared_file"
              ],
              "ending": "wasm"
            }"#,
          )
          .add_config_section(
            "testProcessPlugin",
            r#"{
              "associations": [
                "shared_file"
              ],
              "non-existent": "value"
            }"#,
          );
      })
      .write_file("/test.txt", "text")
      .write_file("/shared_file", "text")
      .build();

    let error_message = run_test_cli(vec!["fmt", "--config", "/config.json"], &environment).err().unwrap();

    assert_eq!(error_message.to_string(), "Had 1 error(s) formatting.");
    assert_eq!(
      environment.take_stderr_messages(),
      vec![
        "[test-process-plugin]: Unknown property in configuration (non-existent)",
        "[test-process-plugin]: Error initializing from configuration file. Had 1 diagnostic(s)."
      ]
    );
    assert_eq!(environment.read_file("/test.txt").unwrap(), "text");
    assert_eq!(environment.read_file("/shared_file").unwrap(), "text");
  }

  #[test]
  fn should_error_when_no_plugins_specified() {
    let environment = TestEnvironmentBuilder::new()
      .with_default_config(|c| {
        c.ensure_plugins_section();
      })
      .write_file("/test.txt", "test")
      .build();

    let err = run_test_cli(vec!["fmt", "**/*.txt"], &environment).err().unwrap();
    err.assert_exit_code(13);

    assert_eq!(
      err.to_string(),
      "No formatting plugins found. Ensure at least one is specified in the 'plugins' array of the configuration file."
    );
  }

  #[test]
  fn should_use_plugins_specified_in_cli_args() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_plugin("https://plugins.dprint.dev/other.wasm");
      })
      .write_file("/test.txt", "test")
      .build();

    run_test_cli(
      vec!["fmt", "**/*.txt", "--plugins", "https://plugins.dprint.dev/test-plugin.wasm"],
      &environment,
    )
    .unwrap();

    assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);
  }

  #[test]
  fn should_allow_using_no_config_when_plugins_specified() {
    let environment = TestEnvironmentBuilder::new().add_remote_wasm_plugin().write_file("/test.txt", "test").build();

    run_test_cli(
      vec!["fmt", "**/*.txt", "--plugins", "https://plugins.dprint.dev/test-plugin.wasm"],
      &environment,
    )
    .unwrap();

    assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);
    assert_eq!(
      environment.take_stderr_messages(),
      vec!["Compiling https://plugins.dprint.dev/test-plugin.wasm"]
    );
  }

  #[test]
  fn should_not_do_excess_object_property_diagnostics_when_plugins_cli_specified() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin()
      .with_default_config(|c| {
        c.add_config_section("excess-object", "{}").add_remote_process_plugin();
      })
      .write_file("/test.txt", "test")
      .build();

    run_test_cli(
      vec!["fmt", "**/*.txt", "--plugins", "https://plugins.dprint.dev/test-plugin.wasm"],
      &environment,
    )
    .unwrap();

    assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);

    // now it errors because no --plugins specified
    let err = run_test_cli(vec!["fmt", "**/*.txt"], &environment).err().unwrap();
    err.assert_exit_code(11);
    assert_eq!(
      err.to_string(),
      "* Unexpected non-string, boolean, or int property (excess-object)\n\nHad 1 config diagnostic(s) in /dprint.json"
    );
  }

  #[test]
  fn should_not_do_excess_primitive_property_diagnostics_when_plugins_cli_specified() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin()
      .with_default_config(|c| {
        c.add_config_section("excess-primitive", "true").add_remote_process_plugin();
      })
      .write_file("/test.txt", "test")
      .build();

    run_test_cli(
      vec!["fmt", "**/*.txt", "--plugins", "https://plugins.dprint.dev/test-plugin.wasm"],
      &environment,
    )
    .unwrap();

    assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);

    // now it errors because no --plugins specified
    let err = run_test_cli(vec!["fmt", "**/*.txt"], &environment).err().unwrap();
    err.assert_exit_code(11);
    assert_eq!(
      err.to_string(),
      "* Unknown property in configuration (excess-primitive)\n\nHad 1 config diagnostic(s) in /dprint.json"
    );
  }

  #[test]
  fn should_error_when_no_files_match_glob() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin().build();
    let error = run_test_cli(vec!["fmt", "**/*.txt"], &environment).err().unwrap();

    assert_no_files_found(&error, &environment);
  }

  #[test]
  fn should_not_error_when_no_files_match_allow_no_files_output() {
    run_allow_no_files_test("fmt");
    run_allow_no_files_test("check");
    run_allow_no_files_test("output-format-times");

    fn run_allow_no_files_test(sub_command: &str) {
      // with
      {
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin().build();
        assert!(run_test_cli(vec![sub_command, "--allow-no-files", "**/*.txt"], &environment).is_ok());
      }
      // without
      {
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin().build();
        let error = run_test_cli(vec![sub_command, "**/*.txt"], &environment).err().unwrap();
        assert_no_files_found(&error, &environment);
      }
    }
  }

  #[track_caller]
  fn assert_no_files_found(error: &TestAppError, _environment: &TestEnvironment) {
    assert_eq!(
      error.to_string(),
      concat!(
        "No files found to format with the specified plugins at /. ",
        "You may want to try using `dprint output-file-paths` to see which files it's finding or run with `--allow-no-files`."
      )
    );
    error.assert_exit_code(14);
  }

  #[cfg(target_os = "windows")]
  #[test]
  fn should_format_absolute_paths_on_windows() {
    let file_path = "D:\\test\\other\\asdf\\file1.txt"; // needs to be in the base directory
    let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
      .with_local_config("D:\\test\\other\\dprint.json", |c| {
        c.add_includes("asdf/**/*.txt").add_remote_wasm_plugin();
      })
      .write_file(file_path, "text1")
      .set_cwd("D:\\test\\other\\")
      .initialize()
      .build();

    // formats because the file path is explicitly provided
    run_test_cli(vec!["fmt", "--", file_path], &environment).unwrap();

    assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);
    assert_eq!(environment.read_file(&file_path).unwrap(), "text1_formatted");
  }

  #[cfg(unix)]
  #[test]
  fn should_format_absolute_paths_on_unix() {
    let file_path = "/test/other/asdf/file1.txt"; // needs to be in the base directory
    let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
      .with_local_config("/test/other/dprint.json", |c| {
        c.add_includes("asdf/**/*.txt").add_remote_wasm_plugin();
      })
      .write_file(&file_path, "text1")
      .set_cwd("/test/other/")
      .initialize()
      .build();

    run_test_cli(vec!["fmt", "--", "/test/other/asdf/file1.txt"], &environment).unwrap();

    assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);
    assert_eq!(environment.read_file(&file_path).unwrap(), "text1_formatted");
  }

  #[test]
  fn should_format_files_with_specific_config_includes() {
    let file_path1 = "/file1.txt";
    let file_path2 = "/file2.txt";
    let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
      .write_file(file_path1, "text1")
      .write_file(file_path2, "text2")
      .with_default_config(|c| {
        c.add_includes("**/file2.txt").add_remote_wasm_plugin();
      })
      .initialize()
      .build();

    run_test_cli(vec!["fmt"], &environment).unwrap();

    assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);
    assert_eq!(environment.read_file(&file_path1).unwrap(), "text1");
    assert_eq!(environment.read_file(&file_path2).unwrap(), "text2_formatted");
  }

  #[test]
  fn should_format_files_with_and_without_config_includes() {
    let file_path1 = "/file1.txt";
    let file_path2 = "/file2.txt";
    for includes in [Some("**/*.txt"), None] {
      let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
        .write_file(file_path1, "text1")
        .write_file(file_path2, "text2")
        .with_default_config(|c| {
          c.add_remote_wasm_plugin();
          if let Some(includes) = includes {
            c.add_includes(includes);
          }
        })
        .initialize()
        .build();

      run_test_cli(vec!["fmt"], &environment).unwrap();

      assert_eq!(environment.take_stdout_messages(), vec![get_plural_formatted_text(2)]);
      assert_eq!(environment.read_file(&file_path1).unwrap(), "text1_formatted");
      assert_eq!(environment.read_file(&file_path2).unwrap(), "text2_formatted");
    }
  }

  #[cfg(target_os = "windows")]
  #[test]
  fn should_format_files_with_config_includes_when_using_back_slashes() {
    let file_path1 = "/file1.txt";
    let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_includes("**\\\\*.txt") // escape for the json
          .add_remote_wasm_plugin();
      })
      .write_file(file_path1, "text1")
      .initialize()
      .build();

    run_test_cli(vec!["fmt"], &environment).unwrap();

    assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);
    assert_eq!(environment.read_file(&file_path1).unwrap(), "text1_formatted");
  }

  #[test]
  fn should_override_config_includes_with_cli_includes() {
    let file_path1 = "/file1.txt";
    let file_path2 = "/file2.txt";
    for includes in [Some("**/*.txt"), None] {
      let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
        .write_file(&file_path1, "text1")
        .write_file(&file_path2, "text2")
        .with_default_config(|c| {
          c.add_remote_wasm_plugin();
          if let Some(includes) = includes {
            c.add_includes(includes);
          }
        })
        .build();

      run_test_cli(vec!["fmt", "/file1.txt"], &environment).unwrap();

      assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);
      assert_eq!(environment.read_file(&file_path1).unwrap(), "text1_formatted");
      assert_eq!(environment.read_file(&file_path2).unwrap(), "text2");
    }
  }

  #[test]
  fn should_combine_config_excludes_with_cli_excludes() {
    let file_path1 = "/file1.txt";
    let file_path2 = "/file2.txt";
    let file_path3 = "/file3.txt";
    let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
      .write_file(&file_path1, "text1")
      .write_file(&file_path2, "text2")
      .write_file(&file_path3, "text3")
      .with_default_config(|c| {
        c.add_excludes("/file1.txt").add_remote_wasm_plugin();
      })
      .initialize()
      .build();

    run_test_cli(vec!["fmt", "--excludes", "/file2.txt"], &environment).unwrap();

    assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);
    assert_eq!(environment.read_file(&file_path1).unwrap(), "text1");
    assert_eq!(environment.read_file(&file_path2).unwrap(), "text2");
    assert_eq!(environment.read_file(&file_path3).unwrap(), "text3_formatted");
  }

  #[test]
  fn should_override_config_excludes_with_cli_excludes_override() {
    let file_path1 = "/file1.txt";
    let file_path2 = "/file2.txt";
    let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
      .write_file(&file_path1, "text1")
      .write_file(&file_path2, "text2")
      .with_default_config(|c| {
        c.add_excludes("/file1.txt").add_remote_wasm_plugin();
      })
      .initialize()
      .build();

    run_test_cli(vec!["fmt", "--excludes-override", "/file2.txt"], &environment).unwrap();

    assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);
    assert_eq!(environment.read_file(&file_path1).unwrap(), "text1_formatted");
    assert_eq!(environment.read_file(&file_path2).unwrap(), "text2");
  }

  #[test]
  fn should_support_clearing_config_excludes_with_cli_excludes_override_arg() {
    let file_path1 = "/file1.txt";
    let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
      .write_file(&file_path1, "text1")
      .with_default_config(|c| {
        c.add_excludes("/file1.txt").add_remote_wasm_plugin();
      })
      .initialize()
      .build();

    run_test_cli(vec!["fmt", "--excludes-override="], &environment).unwrap();

    assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);
    assert_eq!(environment.read_file(&file_path1).unwrap(), "text1_formatted");
  }

  #[test]
  fn should_not_format_explicitly_specified_file_when_excluded() {
    let file_path1 = "/file1.txt";
    let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
      .write_file(&file_path1, "text1")
      .with_default_config(|c| {
        c.add_excludes("/file1.txt").add_remote_wasm_plugin();
      })
      .initialize()
      .build();

    // this is done for tools like lint staged
    let error = run_test_cli(vec!["fmt", "file1.txt"], &environment).err().unwrap();
    assert_no_files_found(&error, &environment);
  }

  #[test]
  fn should_combine_config_excludes_with_cli_args() {
    let file_path1 = "/file1.txt";
    let file_path2 = "/sub/file2.txt";
    let file_path3 = "/sub/file3.txt";
    let file_path4 = "/sub/file4.txt";
    let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
      .write_file(&file_path1, "text1")
      .write_file(&file_path2, "text2")
      .write_file(&file_path3, "text3")
      .write_file(&file_path4, "text4")
      .with_default_config(|c| {
        c.add_includes("/sub/**/*.txt").add_excludes("/sub/file4.txt").add_remote_wasm_plugin();
      })
      .initialize()
      .build();
    run_test_cli(vec!["fmt", "--excludes", "/sub/file3.txt"], &environment).unwrap();

    assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);
    assert_eq!(environment.read_file(&file_path1).unwrap(), "text1");
    assert_eq!(environment.read_file(&file_path2).unwrap(), "text2_formatted");
    assert_eq!(environment.read_file(&file_path3).unwrap(), "text3");
    assert_eq!(environment.read_file(&file_path4).unwrap(), "text4");
  }

  #[test]
  fn should_format_intersect_of_config_includes_and_cli_includes() {
    let file_path1 = "/sub/file1.txt";
    let file_path2 = "/sub/file2.txt";
    let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
      .write_file(&file_path1, "text1")
      .write_file(&file_path2, "text2")
      .with_default_config(|c| {
        c.add_includes("/sub/**/*.txt").add_remote_wasm_plugin();
      })
      .initialize()
      .build();
    run_test_cli(vec!["fmt", "/sub/file2.txt"], &environment).unwrap();

    assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);
    assert_eq!(environment.read_file(&file_path1).unwrap(), "text1");
    assert_eq!(environment.read_file(&file_path2).unwrap(), "text2_formatted");
  }

  #[test]
  fn should_override_config_includes_and_excludes_with_cli_overrides() {
    let file_path1 = "/file1.txt";
    let file_path2 = "/file2.txt";
    let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
      .write_file(&file_path1, "text1")
      .write_file(&file_path2, "text2")
      .with_default_config(|c| {
        c.add_includes("/file2.txt").add_excludes("/file1.txt").add_remote_wasm_plugin();
      })
      .initialize()
      .build();
    run_test_cli(
      vec!["fmt", "--includes-override", "/file1.txt", "--excludes-override", "/file2.txt"],
      &environment,
    )
    .unwrap();

    assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);
    assert_eq!(environment.read_file(&file_path1).unwrap(), "text1_formatted");
    assert_eq!(environment.read_file(&file_path2).unwrap(), "text2");
  }

  #[test]
  fn should_format_files_with_config_excludes() {
    let file_path1 = "/file1.txt";
    let file_path2 = "/file2.txt";
    let file_path3 = "/file3.txt";
    let sub_dir_file_path2 = "/sub-dir/file2.txt";
    let sub_dir_file_path3 = "/sub-dir/file3.txt";
    let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
      .write_file(file_path1, "text")
      .write_file(file_path2, "text")
      .write_file(file_path3, "text")
      .write_file(sub_dir_file_path2, "text")
      .write_file(sub_dir_file_path3, "text")
      .with_default_config(|c| {
        c.add_excludes("/file2.txt").add_excludes("file3.txt").add_remote_wasm_plugin();
      })
      .initialize()
      .build();

    run_test_cli(vec!["fmt"], &environment).unwrap();

    assert_eq!(environment.take_stdout_messages(), vec![get_plural_formatted_text(2)]);
    assert_eq!(environment.read_file(&file_path1).unwrap(), "text_formatted");
    assert_eq!(environment.read_file(&file_path2).unwrap(), "text");
    assert_eq!(environment.read_file(&file_path3).unwrap(), "text");
    assert_eq!(environment.read_file(&sub_dir_file_path2).unwrap(), "text_formatted");
    assert_eq!(environment.read_file(&sub_dir_file_path3).unwrap(), "text");
  }

  #[test]
  fn should_ignore_path_in_config_includes_if_not_exists() {
    let file_path1 = "/file1.txt";
    let file_path2 = "/file2.txt";
    let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
      .write_file(file_path1, "text1")
      .write_file(file_path2, "text2")
      .with_default_config(|c| {
        c.add_includes("/file2.txt").add_includes("/file3.txt").add_remote_wasm_plugin();
      })
      .initialize()
      .build();

    run_test_cli(vec!["fmt"], &environment).unwrap();

    assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);
  }

  #[test]
  fn should_ignore_path_in_cli_includes_arg_even_if_not_exists_since_pattern() {
    let file_path1 = "/file1.txt";
    let file_path2 = "/file2.txt";
    let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
      .write_file(file_path1, "text1")
      .write_file(file_path2, "text2")
      .with_default_config(|c| {
        c.add_remote_wasm_plugin();
      })
      .initialize()
      .build();

    run_test_cli(vec!["fmt", "/file2.txt", "/file3.txt"], &environment).unwrap();

    assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);
  }

  #[test]
  fn should_format_using_hidden_config_file_name() {
    let file_path = "/test/other/file.txt";
    let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
      .with_local_config("/.dprint.json", |c| {
        c.add_remote_wasm_plugin();
      })
      .set_cwd("/test/other/")
      .write_file(file_path, "text")
      .build();

    run_test_cli(vec!["fmt"], &environment).unwrap();
    assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);
    assert_eq!(
      environment.take_stderr_messages(),
      vec!["Compiling https://plugins.dprint.dev/test-plugin.wasm"]
    );
    assert_eq!(environment.read_file(&file_path).unwrap(), "text_formatted");
  }

  #[test]
  fn should_format_using_config_in_ancestor_directory() {
    let config_file_names = vec!["dprint.json", "dprint.jsonc", ".dprint.json", ".dprint.jsonc"];
    for config_file_name in config_file_names {
      let file_path = "/test/other/file.txt";
      let environment = TestEnvironmentBuilder::new()
        .add_remote_wasm_plugin()
        .with_local_config(format!("/{}", config_file_name), |config_file| {
          config_file.add_remote_wasm_plugin().add_includes("**/*.txt");
        })
        .initialize()
        .write_file(&file_path, "text")
        .build();
      environment.set_cwd("/test/other/");
      run_test_cli(vec!["fmt"], &environment).unwrap();
      assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);
      assert_eq!(environment.take_stderr_messages(), Vec::<String>::new());
      assert_eq!(environment.read_file(&file_path).unwrap(), "text_formatted");
    }
  }

  #[test]
  fn should_format_incrementally_when_specified_on_cli() {
    let file_path1 = "/subdir/file1.txt";
    let no_change_msg = "No change: /subdir/file1.txt";
    let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin();
      })
      .write_file(&file_path1, "text1")
      .initialize()
      .build();

    // this is now the default
    run_test_cli(vec!["fmt", "--incremental"], &environment).unwrap();

    assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);
    assert_eq!(environment.read_file(&file_path1).unwrap(), "text1_formatted");

    environment.clear_logs();
    run_test_cli(vec!["fmt", "--incremental", "--log-level=debug"], &environment).unwrap();
    assert_eq!(environment.take_stderr_messages().iter().any(|msg| msg.contains(no_change_msg)), true);

    // update the file and ensure it's formatted
    environment.write_file(&file_path1, "asdf").unwrap();
    environment.clear_logs();
    run_test_cli(vec!["fmt", "--incremental"], &environment).unwrap();
    assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);
    assert_eq!(environment.read_file(&file_path1).unwrap(), "asdf_formatted");

    // update the global config and ensure it's formatted
    environment
      .write_file(
        "./dprint.json",
        r#"{
            "indentWidth": 2,
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#,
      )
      .unwrap();
    environment.clear_logs();
    run_test_cli(vec!["fmt", "--incremental", "--log-level=debug"], &environment).unwrap();
    assert_eq!(environment.take_stderr_messages().iter().any(|msg| msg.contains(no_change_msg)), false);

    // update the plugin config and ensure it's formatted
    environment
      .write_file(
        "./dprint.json",
        r#"{
            "indentWidth": 2,
            "test-plugin": {
                "ending": "custom-formatted",
                "line_width": 80
            },
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#,
      )
      .unwrap();
    environment.clear_logs();
    run_test_cli(vec!["fmt", "--incremental"], &environment).unwrap();
    assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);
    assert_eq!(environment.read_file(&file_path1).unwrap(), "asdf_formatted_custom-formatted");

    // Try this a few times. There was a bug where the config hashmap was being serialized causing
    // random order and the hash to be new each time.
    for _ in 1..4 {
      environment.clear_logs();
      run_test_cli(vec!["fmt", "--incremental", "--log-level=debug"], &environment).unwrap();
      assert_eq!(environment.take_stderr_messages().iter().any(|msg| msg.contains(no_change_msg)), true);
    }

    // change the cwd and ensure it's not formatted again
    environment.clear_logs();
    environment.set_cwd("/subdir");
    run_test_cli(vec!["fmt", "--incremental", "--log-level=debug"], &environment).unwrap();
    assert_eq!(
      environment
        .take_stderr_messages()
        .iter()
        .any(|msg| msg.contains("No change: /subdir/file1.txt")),
      true
    );
  }

  #[test]
  fn should_format_incrementally_when_specified_via_config() {
    let file_path1 = "/file1.txt";
    let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin().set_incremental(true);
      })
      .initialize()
      .write_file(&file_path1, "text1")
      .build();

    run_test_cli(vec!["fmt"], &environment).unwrap();

    assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);
    assert_eq!(environment.read_file(&file_path1).unwrap(), "text1_formatted");

    environment.clear_logs();
    run_test_cli(vec!["fmt", "--log-level=debug"], &environment).unwrap();
    assert_eq!(environment.take_stderr_messages().iter().any(|msg| msg.contains("No change: /file1.txt")), true);
  }

  #[test]
  fn incremental_should_error_for_unstable_format() {
    let file_path1 = "/file1.txt";
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
      .write_file(&file_path1, "unstable_fmt_true")
      .build();

    let result = run_test_cli(vec!["fmt", "--incremental"], &environment).err().unwrap();

    assert_eq!(result.to_string(), "Had 1 error(s) formatting.");
    assert_eq!(
      environment.take_stderr_messages(),
      vec![concat!(
        "Error formatting /file1.txt. Message: Formatting not stable. Bailed after 5 tries. ",
        "This indicates a bug in the plugin where it formats the file differently each time."
      )
      .to_string()]
    );
  }

  #[test]
  fn incremental_should_error_for_unstable_format_that_errors() {
    let file_path1 = "/file1.txt";
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
      .write_file(&file_path1, "unstable_fmt_then_error")
      .build();

    let result = run_test_cli(vec!["fmt", "--incremental"], &environment).err().unwrap();

    assert_eq!(result.to_string(), "Had 1 error(s) formatting.");
    assert_eq!(
      environment.take_stderr_messages(),
      vec![concat!(
        "Error formatting /file1.txt. Message: Formatting succeeded initially, but failed when ensuring a ",
        "stable format. This is most likely a bug in the plugin where the text it produces is not ",
        "syntatically correct. Please report this as a bug to the plugin that formatted this file.\n\n",
        "Did error."
      )
      .to_string()]
    );
  }

  #[test]
  fn should_format_incrementally_with_check() {
    let file_path1 = "/file1.txt";
    let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin();
      })
      .write_file(&file_path1, "text1_formatted")
      .initialize()
      .build();

    run_test_cli(vec!["check", "--incremental"], &environment).unwrap();
    run_test_cli(vec!["check", "--incremental"], &environment).unwrap();

    environment.write_file(file_path1, "text1").unwrap();
    let err = run_test_cli(vec!["check", "--incremental"], &environment).unwrap_err();
    err.assert_exit_code(20);

    environment.write_file(file_path1, "text1_formatted").unwrap();
    run_test_cli(vec!["check", "--incremental"], &environment).unwrap();
    run_test_cli(vec!["check", "--incremental"], &environment).unwrap();
    environment.clear_logs();
  }

  #[test]
  fn should_format_without_incremental_when_specified() {
    let file_path1 = "/subdir/file1.txt";
    let no_change_msg = "No change:";
    let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin();
      })
      .write_file(&file_path1, "text1")
      .initialize()
      .build();

    run_test_cli(vec!["fmt", "--incremental=false"], &environment).unwrap();

    assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);
    assert_eq!(environment.read_file(&file_path1).unwrap(), "text1_formatted");

    environment.clear_logs();
    run_test_cli(vec!["fmt", "--incremental=false", "--log-level=debug"], &environment).unwrap();
    assert!(!environment.take_stderr_messages().iter().any(|msg| msg.contains(no_change_msg)));
  }

  #[test]
  fn allow_skipping_stable_format() {
    let file_path1 = "/file1.txt";
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
      .write_file(&file_path1, "unstable_fmt_true")
      .build();

    run_test_cli(vec!["fmt", "--skip-stable-format", "*.txt"], &environment).unwrap();

    assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);
    assert_eq!(environment.read_file(&file_path1).unwrap(), "unstable_fmt_false_formatted");
  }

  #[test]
  fn should_not_output_when_no_files_need_formatting() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
      .write_file("/file.txt", "text_formatted")
      .build();
    run_test_cli(vec!["fmt", "/file.txt"], &environment).unwrap();
  }

  #[test]
  fn should_format_with_diff() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
      .write_file("/file.txt", "const t=4;")
      .build();
    run_test_cli(vec!["fmt", "--diff", "/file.txt"], &environment).unwrap();
    assert_eq!(
      environment.take_stdout_messages(),
      vec![
        format!(
          "{}\n{}\n--",
          format!("{} /file.txt:", "from".bold().red().to_string()),
          get_difference("const t=4;", "const t=4;_formatted"),
        ),
        get_singular_formatted_text()
      ]
    );
  }

  #[test]
  fn should_not_output_when_no_files_need_formatting_for_check() {
    let file_path = "/file.txt";
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
      .write_file(&file_path, "text_formatted")
      .build();
    run_test_cli(vec!["check", "/file.txt"], &environment).unwrap();
  }

  #[test]
  fn should_output_when_a_file_need_formatting_for_check() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
      .write_file("/file.txt", "const t=4;")
      .build();
    let err = run_test_cli(vec!["check", "/file.txt"], &environment).unwrap_err();
    err.assert_exit_code(20);
    assert_eq!(err.to_string(), get_singular_check_text());
    assert_eq!(
      environment.take_stdout_messages(),
      vec![format!(
        "{}\n{}\n--",
        format!("{} /file.txt:", "from".bold().red().to_string()),
        get_difference("const t=4;", "const t=4;_formatted"),
      ),]
    );
  }

  #[test]
  fn should_output_when_files_need_formatting_for_check() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
      .write_file("/file1.txt", "const t=4;")
      .write_file("/file2.txt", "const t=5;")
      .build();

    let err = run_test_cli(vec!["check", "/file1.txt", "/file2.txt"], &environment).unwrap_err();
    err.assert_exit_code(20);
    assert_eq!(err.to_string(), get_plural_check_text(2));
    let mut logged_messages = environment.take_stdout_messages();
    logged_messages.sort(); // the order is not deterministic
    assert_eq!(
      logged_messages,
      vec![
        format!(
          "{}\n{}\n--",
          format!("{} /file1.txt:", "from".bold().red().to_string()),
          get_difference("const t=4;", "const t=4;_formatted"),
        ),
        format!(
          "{}\n{}\n--",
          format!("{} /file2.txt:", "from".bold().red().to_string()),
          get_difference("const t=5;", "const t=5;_formatted"),
        ),
      ]
    );
  }

  #[test]
  fn should_output_list_different_when_files_need_formatting_for_check() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
      .write_file("/file1.txt", "const t=4;")
      .write_file("/file2.txt", "const t=5;")
      .build();

    let err = run_test_cli(vec!["check", "--list-different", "/file1.txt", "/file2.txt"], &environment).unwrap_err();
    err.assert_exit_code(20);
    assert_eq!(err.to_string(), ""); // no output because we outputted the files
    let mut logged_messages = environment.take_stdout_messages();
    logged_messages.sort(); // the order is not deterministic
    assert_eq!(logged_messages, vec!["/file1.txt", "/file2.txt",]);
  }

  #[test]
  fn should_handle_bom() {
    let file_path = "/file.txt";
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
      .write_file(&file_path, "\u{FEFF}text")
      .build();
    run_test_cli(vec!["fmt", "/file.txt"], &environment).unwrap();
    assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);
    assert_eq!(environment.read_file(&file_path).unwrap(), "\u{FEFF}text_formatted");
  }

  #[test]
  fn should_format_for_stdin_fmt_with_file_name() {
    // it should not output anything when downloading plugins
    let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_includes("/test/**.txt").add_remote_wasm_plugin();
      })
      .build();

    let test_std_in = TestStdInReader::from("text");
    run_test_cli_with_stdin(vec!["fmt", "--stdin", "file.txt"], &environment, test_std_in).unwrap();
    // should format even though it wasn't matched because an absolute path wasn't provided
    assert_eq!(environment.take_stdout_messages(), vec!["text_formatted"]);
    assert_eq!(
      environment.take_stderr_messages(),
      vec!["Compiling https://plugins.dprint.dev/test-plugin.wasm"]
    );
  }

  #[test]
  fn should_format_for_stdin_fmt_with_extension() {
    let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_includes("/test/**.txt").add_remote_wasm_plugin();
      })
      .build();

    let test_std_in = TestStdInReader::from("text");
    run_test_cli_with_stdin(vec!["fmt", "--stdin", "txt"], &environment, test_std_in).unwrap();
    // should format even though it wasn't matched because an absolute path wasn't provided
    assert_eq!(environment.take_stdout_messages(), vec!["text_formatted"]);
    assert_eq!(
      environment.take_stderr_messages(),
      vec!["Compiling https://plugins.dprint.dev/test-plugin.wasm"]
    );

    // now try with a leading period
    let test_std_in = TestStdInReader::from("text");
    run_test_cli_with_stdin(vec!["fmt", "--stdin", ".txt"], &environment, test_std_in).unwrap();
    assert_eq!(environment.take_stdout_messages(), vec!["text_formatted"]);
  }

  #[test]
  fn should_stdin_fmt_calling_other_plugin() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin().build();
    let test_std_in = TestStdInReader::from("plugin: format this text");
    run_test_cli_with_stdin(vec!["fmt", "--stdin", "file.txt"], &environment, test_std_in).unwrap();
    assert_eq!(environment.take_stdout_messages(), vec!["plugin: format this text_formatted_process_formatted"]);
  }

  #[test]
  fn should_handle_error_for_stdin_fmt() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin();
      })
      .build(); // don't initialize
    let test_std_in = TestStdInReader::from("should_error");
    let error_message = run_test_cli_with_stdin(vec!["fmt", "--stdin", "file.txt"], &environment, test_std_in)
      .err()
      .unwrap();
    assert_eq!(error_message.to_string(), "Did error.");
    assert_eq!(
      environment.take_stderr_messages(),
      vec!["Compiling https://plugins.dprint.dev/test-plugin.wasm"]
    );
  }

  #[test]
  fn should_format_for_stdin_with_absolute_paths() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_includes("/src/**.*").add_remote_wasm_plugin();
      })
      .write_file("/file.txt", "")
      .write_file("/src/file.txt", "")
      .build();
    // not matching file
    let test_std_in = TestStdInReader::from("text");
    run_test_cli_with_stdin(vec!["fmt", "--stdin", "/file.txt"], &environment, test_std_in.clone()).unwrap();
    assert_eq!(environment.take_stdout_messages(), vec!["text"]);

    // matching file
    run_test_cli_with_stdin(vec!["fmt", "--stdin", "/src/file.txt"], &environment, test_std_in.clone()).unwrap();
    assert_eq!(environment.take_stdout_messages(), vec!["text_formatted"]);

    // override what's in the config when overriding
    run_test_cli_with_stdin(
      vec!["fmt", "--stdin", "/file.txt", "--includes-override", "**/*.txt"],
      &environment,
      test_std_in,
    )
    .unwrap();
    assert_eq!(environment.take_stdout_messages(), vec!["text_formatted"]);
  }

  #[test]
  fn should_not_format_stdin_resolving_config_file_from_provided_path_when_relative() {
    let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_includes("./**/*.txt").add_remote_wasm_plugin();
      })
      .with_local_config("./sub-dir/dprint.json", |c| {
        c.add_includes("./**/*.txt")
          .add_remote_wasm_plugin()
          .add_config_section("test-plugin", r#"{ "ending": "new_ending" }"#);
      })
      .build();
    let test_std_in = TestStdInReader::from("text");
    run_test_cli_with_stdin(vec!["fmt", "--stdin", "sub-dir/file.txt"], &environment, test_std_in).unwrap();
    // Should use cwd since the absolute path wasn't provided. In order to use the proper config file,
    // the absolute path must be provided instead of a relative one in order to properly pick up
    // inclusion/exclusion rules and the proper configuration file.
    assert_eq!(environment.take_stdout_messages(), vec!["text_formatted"]);
    assert_eq!(
      environment.take_stderr_messages(),
      vec!["Compiling https://plugins.dprint.dev/test-plugin.wasm"]
    );
  }

  #[test]
  fn should_format_stdin_resolving_config_file_from_provided_path_when_absolute() {
    let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_includes("./**/*.txt").add_remote_wasm_plugin();
      })
      .with_local_config("/sub-dir/dprint.json", |c| {
        c.add_includes("./**/*.txt")
          .add_remote_wasm_plugin()
          .add_config_section("test-plugin", r#"{ "ending": "new_ending" }"#);
      })
      .write_file("/sub-dir/file.txt", "test")
      .initialize()
      .build();
    let test_std_in = TestStdInReader::from("text");
    run_test_cli_with_stdin(vec!["fmt", "--stdin", "/sub-dir/file.txt"], &environment, test_std_in).unwrap();
    assert_eq!(environment.take_stdout_messages(), vec!["text_new_ending"]);
  }

  #[test]
  fn should_error_if_process_plugin_has_no_checksum_in_config() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_process_plugin()
      .with_default_config(|c| {
        c.clear_plugins().add_plugin("https://plugins.dprint.dev/test-process.json");
      })
      .write_file("/test.txt_ps", "")
      .build();
    let err = run_test_cli(vec!["fmt", "*.*"], &environment).err().unwrap();
    err.assert_exit_code(12);
    let actual_plugin_file_checksum = TestProcessPluginFile::default().checksum();

    assert_eq!(
      err.to_string(),
      format!(
        concat!(
          "Error resolving plugin https://plugins.dprint.dev/test-process.json: The plugin must have a checksum specified ",
          "for security reasons since it is not a Wasm plugin. Check the plugin's release notes for what the checksum is or if ",
          "you trust the source, you may specify: https://plugins.dprint.dev/test-process.json@{}"
        ),
        actual_plugin_file_checksum,
      ),
    );
  }

  #[test]
  fn should_error_if_process_plugin_has_wrong_checksum_in_config() {
    let environment = TestEnvironmentBuilder::with_remote_process_plugin()
      .with_default_config(|c| {
        c.add_remote_process_plugin_with_checksum("asdf");
      })
      .write_file("/test.txt_ps", "")
      .build();
    let actual_plugin_file_checksum = TestProcessPluginFile::default().checksum();
    let err = run_test_cli(vec!["fmt", "*.*"], &environment).err().unwrap();
    err.assert_exit_code(12);

    assert_eq!(
      err.to_string(),
      format!(
        concat!(
          "Error resolving plugin https://plugins.dprint.dev/test-process.json: Invalid checksum specified ",
          "in configuration file. Check the plugin's release notes for what the expected checksum is.\n\n",
          "The checksum did not match the expected checksum.\n\n",
          "Actual: {}\n",
          "Expected: asdf"
        ),
        actual_plugin_file_checksum,
      )
    );
  }

  #[test]
  fn should_error_if_wasm_plugin_has_wrong_checksum_in_config() {
    let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin_with_checksum("asdf");
      })
      .write_file("/test.txt", "")
      .build();
    let actual_plugin_file_checksum = test_helpers::get_test_wasm_plugin_checksum();
    let err = run_test_cli(vec!["fmt", "*.*"], &environment).err().unwrap();
    err.assert_exit_code(12);

    assert_eq!(
      err.to_string(),
      format!(
        concat!(
          "Error resolving plugin https://plugins.dprint.dev/test-plugin.wasm: Invalid checksum specified ",
          "in configuration file. Check the plugin's release notes for what the expected checksum is.\n\n",
          "The checksum did not match the expected checksum.\n\n",
          "Actual: {}\n",
          "Expected: asdf"
        ),
        actual_plugin_file_checksum,
      )
    );
  }

  #[test]
  fn should_not_error_if_wasm_plugin_has_correct_checksum_in_config() {
    let actual_plugin_file_checksum = test_helpers::get_test_wasm_plugin_checksum();
    let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin_with_checksum(&actual_plugin_file_checksum);
      })
      .write_file("/test.txt", "text")
      .build();
    run_test_cli(vec!["fmt", "*.*"], &environment).unwrap();

    assert_eq!(environment.read_file("/test.txt").unwrap(), "text_formatted");
    assert_eq!(environment.take_stdout_messages(), vec![get_singular_formatted_text()]);
    assert_eq!(
      environment.take_stderr_messages(),
      vec!["Compiling https://plugins.dprint.dev/test-plugin.wasm"]
    );
  }

  #[test]
  fn should_error_if_process_plugin_has_wrong_checksum_in_file_for_zip() {
    let environment = TestEnvironmentBuilder::with_remote_process_plugin()
      .add_remote_process_plugin_at_url(
        "https://plugins.dprint.dev/test-process.json",
        &TestProcessPluginFileBuilder::default().zip_checksum("asdf").build(),
      )
      .with_default_config(|c| {
        c.add_remote_process_plugin();
      })
      .write_file("/test.txt_ps", "")
      .build();
    let actual_plugin_zip_file_checksum = &*PROCESS_PLUGIN_ZIP_CHECKSUM;
    let err = run_test_cli(vec!["fmt", "*.*"], &environment).err().unwrap();
    err.assert_exit_code(12);

    assert_eq!(
      err.to_string(),
      format!(
        concat!(
          "Error resolving plugin https://plugins.dprint.dev/test-process.json: Invalid checksum found ",
          "within process plugin's manifest file for 'https://github.com/dprint/test-process-plugin/releases/0.1.0/test-process-plugin.zip'. ",
          "This is likely a bug in the process plugin. Please report it.\n\n",
          "The checksum did not match the expected checksum.\n\n",
          "Actual: {}\n",
          "Expected: asdf"
        ),
        actual_plugin_zip_file_checksum,
      )
    );
  }

  #[test]
  fn should_format_many_files() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin().build();
    for i in 0..100 {
      let file_path = format!("/file{}.txt", i);
      environment.write_file(&file_path, &format!("text{}", i)).unwrap();
    }
    for i in 0..100 {
      let file_path = format!("/file{}.txt_ps", i);
      environment.write_file(&file_path, &format!("text{}", i)).unwrap();
    }

    run_test_cli(vec!["fmt", "/*.*"], &environment).unwrap();
    assert_eq!(environment.take_stdout_messages(), vec![get_plural_formatted_text(200)]);

    for i in 0..100 {
      let file_path = format!("/file{}.txt", i);
      assert_eq!(environment.read_file(&file_path).unwrap(), format!("text{}_formatted", i));
    }
    for i in 0..100 {
      let file_path = format!("/file{}.txt_ps", i);
      assert_eq!(environment.read_file(&file_path).unwrap(), format!("text{}_formatted_process", i));
    }
  }

  #[test]
  fn should_error_once_on_config_diagnostic_many_files() {
    // configuration diagnostic should only be shown by one thread
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin().build();
    environment
      .write_file(
        "./dprint.json",
        r#"{
            "test-plugin": { "non-existent": 25 },
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#,
      )
      .unwrap();

    for i in 0..100 {
      let file_path = format!("/file{}.txt", i);
      environment.write_file(&file_path, &format!("text{}", i)).unwrap();
    }

    let error_message = run_test_cli(vec!["fmt", "**/*.txt"], &environment).err().unwrap();

    assert_eq!(error_message.to_string(), "Had 1 error(s) formatting.");
    assert_eq!(
      environment.take_stderr_messages(),
      vec![
        "[test-plugin]: Unknown property in configuration (non-existent)",
        "[test-plugin]: Error initializing from configuration file. Had 1 diagnostic(s)."
      ]
    );
  }

  #[test]
  fn should_format_with_nested_config_calling_other_plugin() {
    let file_path1 = "/file.txt";
    let file_path2 = "/sub_dir/file.txt";
    let file_path3 = "/sub_dir/ignored/file.txt";
    let file_path4 = "/sub_dir/more/file.txt";
    let file_path5 = "/sub_dir/more/ignored.txt_ps";
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin()
      .with_local_config("/sub_dir/dprint.json", |config| {
        config
          .add_remote_wasm_plugin()
          .add_remote_process_plugin()
          .add_excludes("./ignored")
          .add_config_section("test-plugin", r#"{ "ending": "custom-formatted1" }"#)
          .add_config_section("testProcessPlugin", r#"{ "ending": "custom-formatted2" }"#);
      })
      .with_local_config("/sub_dir/more/dprint.json", |config| {
        config.add_remote_wasm_plugin();
      })
      .write_file(&file_path1, "plugin: plugin: format this text")
      .write_file(&file_path2, "plugin: plugin: format this other text")
      .write_file(&file_path3, "ignored")
      .write_file(&file_path4, "text")
      .write_file(&file_path5, "ignored")
      .build();
    run_test_cli(vec!["fmt"], &environment).unwrap();
    assert_eq!(environment.take_stdout_messages(), vec![get_plural_formatted_text(3)]);
    assert_eq!(
      environment.read_file(&file_path1).unwrap(),
      "plugin: plugin: format this text_formatted_formatted_process_formatted"
    );
    assert_eq!(
      environment.read_file(&file_path2).unwrap(),
      "plugin: plugin: format this other text_custom-formatted1_custom-formatted2_custom-formatted1"
    );
    assert_eq!(environment.read_file(&file_path3).unwrap(), "ignored");
    assert_eq!(environment.read_file(&file_path4).unwrap(), "text_formatted");
  }

  #[test]
  fn should_not_error_nested_config_no_matching_files_in_scope_cli_args() {
    let file_path1 = "/sub_dir/file.txt";
    let file_path2 = "/sub_dir/sub_dir/file.txt";
    let file_path3 = "/sub_dir/more/ignored.txt_ps";
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin()
      .with_local_config("/sub_dir/dprint.json", |config| {
        config.add_remote_wasm_plugin();
      })
      .with_local_config("/sub_dir/more/dprint.json", |config| {
        config.add_remote_process_plugin();
      })
      .write_file(&file_path1, "here1")
      .write_file(&file_path2, "here2")
      .write_file(&file_path3, "here3")
      .build();
    // previously this was erroring in the sub directory
    run_test_cli(vec!["fmt", "**/*.txt"], &environment).unwrap();
    assert_eq!(environment.take_stdout_messages(), vec![get_plural_formatted_text(2)]);
    assert_eq!(environment.read_file(&file_path1).unwrap(), "here1_formatted");
    assert_eq!(environment.read_file(&file_path2).unwrap(), "here2_formatted");
    assert_eq!(environment.read_file(&file_path3).unwrap(), "here3");

    // now try with a pattern that doesn't match any file in any scope and it should error
    let err = run_test_cli(vec!["fmt", "**/*.no_matching"], &environment).unwrap_err();
    assert_no_files_found(&err, &environment);
  }

  #[test]
  fn should_error_no_files_sub_dir_config() {
    let file_path1 = "/file.txt";
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin()
      .with_local_config("/sub_dir/dprint.json", |config| {
        config.add_remote_wasm_plugin().add_remote_process_plugin();
      })
      .write_file(&file_path1, "format this text")
      .build();
    let err = run_test_cli(vec!["fmt"], &environment).err().unwrap();
    assert_eq!(
      err.to_string(),
      "No files found to format with the specified plugins at /sub_dir. You may want to try using `dprint output-file-paths` to see which files it's finding or run with `--allow-no-files`."
    );
    err.assert_exit_code(14);
  }
}
