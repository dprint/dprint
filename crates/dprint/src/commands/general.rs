use std::rc::Rc;

use anyhow::Result;

use crate::arg_parser::create_cli_parser;
use crate::arg_parser::CliArgParserKind;
use crate::arg_parser::CliArgs;
use crate::arg_parser::OutputFilePathsSubCommand;
use crate::environment::Environment;
use crate::plugins::PluginResolver;
use crate::resolution::get_plugins_scope_from_args;
use crate::resolution::resolve_plugins_scope_and_paths;
use crate::utils::get_table_text;
use crate::utils::is_out_of_date;

pub fn output_version<TEnvironment: Environment>(environment: &TEnvironment) -> Result<()> {
  log_stdout_info!(environment, "{} {}", env!("CARGO_PKG_NAME"), environment.cli_version());

  Ok(())
}

pub async fn output_help<TEnvironment: Environment>(
  args: &CliArgs,
  environment: &TEnvironment,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
  help_text: &str,
) -> Result<()> {
  // log the cli's help first
  log_stdout_info!(environment, help_text);

  // now check for the plugins
  let scope_result = get_plugins_scope_from_args(args, environment, plugin_resolver).await;
  match scope_result {
    Ok(scope) => {
      if !scope.plugins.is_empty() {
        let table_text = get_table_text(scope.plugins.values().map(|plugin| (plugin.name(), plugin.info().help_url.as_str())).collect());
        log_stdout_info!(environment, "\nPLUGINS HELP:");
        log_stdout_info!(
          environment,
          &console_static_text::strip_ansi_codes(&table_text.render(
            4, // indent
            // don't render taking terminal width into account
            // as these are urls and we want them to be clickable
            None,
          ))
        );
      }
    }
    Err(err) => {
      log_debug!(environment, "Error getting plugins for help. {:#}", err.to_string());
    }
  }

  if let Some(latest_version) = is_out_of_date(environment).await {
    log_stdout_info!(
      environment,
      "\nLatest version: {} (Current is {})\nDownload the latest version by running: dprint upgrade",
      latest_version,
      environment.cli_version(),
    );
  }

  Ok(())
}

pub async fn output_license<TEnvironment: Environment>(
  args: &CliArgs,
  environment: &TEnvironment,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
) -> Result<()> {
  log_stdout_info!(environment, "==== DPRINT CLI LICENSE ====");
  log_stdout_info!(environment, std::str::from_utf8(include_bytes!("../../LICENSE"))?);

  // now check for the plugins
  for plugin in get_plugins_scope_from_args(args, environment, plugin_resolver).await?.plugins.values() {
    log_stdout_info!(environment, "\n==== {} LICENSE ====", plugin.name().to_uppercase());
    let initialized_plugin = plugin.initialize().await?;
    log_stdout_info!(environment, &initialized_plugin.license_text().await?);
  }

  Ok(())
}

pub fn clear_cache(environment: &impl Environment) -> Result<()> {
  let cache_dir = environment.get_cache_dir();
  environment.remove_dir_all(&cache_dir)?;
  log_stdout_info!(environment, "Deleted {}", cache_dir.display());
  Ok(())
}

pub async fn output_file_paths<TEnvironment: Environment>(
  cmd: &OutputFilePathsSubCommand,
  args: &CliArgs,
  environment: &TEnvironment,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
) -> Result<()> {
  let scopes = resolve_plugins_scope_and_paths(args, &cmd.patterns, environment, plugin_resolver).await?;
  let file_paths = scopes.iter().flat_map(|x| x.file_paths_by_plugins.all_file_paths());
  for file_path in file_paths {
    log_stdout_info!(environment, "{}", file_path.display())
  }
  Ok(())
}

pub fn completions<TEnvironment: Environment>(shell: clap_complete::Shell, environment: &TEnvironment) -> Result<()> {
  let mut cmd = create_cli_parser(CliArgParserKind::ForCompletions);

  let mut buffer = Vec::new();
  clap_complete::generate(shell, &mut cmd, "dprint", &mut buffer);
  environment.log_machine_readable(&String::from_utf8_lossy(&buffer));

  Ok(())
}

#[cfg(test)]
mod test {
  use pretty_assertions::assert_eq;

  use crate::environment::Environment;
  use crate::environment::TestEnvironment;
  use crate::environment::TestEnvironmentBuilder;
  use crate::test_helpers::get_expected_help_text;
  use crate::test_helpers::run_test_cli;

  #[test]
  fn should_output_version_with_v() {
    let environment = TestEnvironment::new();
    run_test_cli(vec!["-v"], &environment).unwrap();
    let logged_messages = environment.take_stdout_messages();
    assert_eq!(logged_messages, vec![format!("dprint {}", environment.cli_version())]);
  }

  #[test]
  fn should_output_version_with_no_plugins() {
    let environment = TestEnvironment::new();
    run_test_cli(vec!["--version"], &environment).unwrap();
    let logged_messages = environment.take_stdout_messages();
    assert_eq!(logged_messages, vec![format!("dprint {}", environment.cli_version())]);
  }

  #[test]
  fn should_output_version_and_ignore_plugins() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin().build();
    run_test_cli(vec!["--version"], &environment).unwrap();
    let logged_messages = environment.take_stdout_messages();
    assert_eq!(logged_messages, vec![format!("dprint {}", environment.cli_version())]);
  }

  #[test]
  fn should_output_help_with_no_plugins() {
    let environment = TestEnvironment::new();
    run_test_cli(vec!["--help"], &environment).unwrap();
    let logged_messages = environment.take_stdout_messages();
    assert_eq!(logged_messages, vec![get_expected_help_text()]);
  }

  #[test]
  fn should_output_help_no_sub_commands() {
    let environment = TestEnvironment::new();
    run_test_cli(vec![], &environment).unwrap();
    let logged_messages = environment.take_stdout_messages();
    assert_eq!(logged_messages, vec![get_expected_help_text()]);
  }

  #[test]
  fn should_output_help_with_plugins() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin().build();

    run_test_cli(vec!["--help"], &environment).unwrap();
    assert_eq!(
      environment.take_stdout_messages(),
      vec![
        get_expected_help_text(),
        "\nPLUGINS HELP:",
        "    test-plugin         https://dprint.dev/plugins/test\n    test-process-plugin https://dprint.dev/plugins/test-process"
      ]
    );
  }

  #[test]
  fn should_output_help_when_cli_not_out_of_date() {
    let environment = TestEnvironment::new();
    environment.add_remote_file_bytes("https://plugins.dprint.dev/cli.json", r#"{ "version": "0.0.0" }"#.as_bytes().to_vec());
    run_test_cli(vec!["--help"], &environment).unwrap();
    let logged_messages = environment.take_stdout_messages();
    assert_eq!(logged_messages, vec![get_expected_help_text()]);
  }

  #[test]
  fn should_output_help_when_cli_out_of_date() {
    let environment = TestEnvironment::new();
    environment.add_remote_file_bytes("https://plugins.dprint.dev/cli.json", r#"{ "version": "0.1.0" }"#.as_bytes().to_vec());
    run_test_cli(vec!["--help"], &environment).unwrap();
    let logged_messages = environment.take_stdout_messages();
    assert_eq!(
      logged_messages,
      vec![
        get_expected_help_text(),
        concat!(
          "\nLatest version: 0.1.0 (Current is 0.0.0)",
          "\nDownload the latest version by running: dprint upgrade",
        )
      ]
    );
  }

  #[test]
  fn should_output_resolved_file_paths() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin()
      .write_file("/file.txt", "const t=4;")
      .write_file("/file2.txt", "const t=4;")
      .write_file("/file3.txt_ps", "const t=4;")
      .build();
    run_test_cli(vec!["output-file-paths", "**/*.*"], &environment).unwrap();
    let mut logged_messages = environment.take_stdout_messages();
    logged_messages.sort();
    assert_eq!(logged_messages, vec!["/file.txt", "/file2.txt", "/file3.txt_ps"]);
  }

  #[test]
  fn should_not_output_file_paths_not_supported_by_plugins() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin()
      .write_file("/file.ts", "const t=4;")
      .write_file("/file2.ts", "const t=4;")
      .build();
    run_test_cli(vec!["output-file-paths", "**/*.*"], &environment).unwrap();
    assert_eq!(environment.take_stdout_messages().len(), 0);
  }

  #[test]
  fn should_output_resolved_file_paths_when_using_backslashes() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin()
      .write_file("/file.txt", "const t=4;")
      .write_file("/file2.txt", "const t=4;")
      .write_file("/file3.txt_ps", "const t=4;")
      .build();
    run_test_cli(vec!["output-file-paths", "**\\*.*"], &environment).unwrap();
    let mut logged_messages = environment.take_stdout_messages();
    logged_messages.sort();
    assert_eq!(logged_messages, vec!["/file.txt", "/file2.txt", "/file3.txt_ps"]);
  }

  #[test]
  fn should_output_associations_in_resolved_paths() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_default_config(|config_file| {
        config_file
          .add_includes("**/*.other")
          .add_config_section(
            "test-plugin",
            r#"{
            "associations": [
              "**/*.other"
            ],
            "ending": "wasm"
          }"#,
          )
          .add_remote_wasm_plugin();
      })
      .write_file("/file.txt", "") // won't match because it doesn't match via associations
      .write_file("/file.other", "")
      .initialize()
      .build();
    run_test_cli(vec!["output-file-paths", "**/*.*"], &environment).unwrap();
    let mut logged_messages = environment.take_stdout_messages();
    logged_messages.sort();
    assert_eq!(logged_messages, vec!["/file.other"]);
  }

  #[test]
  fn should_handle_associations_with_only_exclude() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_process_plugin()
      .add_remote_wasm_plugin()
      .with_default_config(|config_file| {
        config_file
          .add_includes("**/*.txt")
          .add_config_section(
            "test-plugin",
            r#"{
            "associations": [
              "!**/exclude/**/*.txt"
            ],
            "ending": "wasm"
          }"#,
          )
          .add_config_section(
            "testProcessPlugin",
            r#"{
            "associations": [
              "!**/exclude/test-process-plugin-exact-file"
            ],
          }"#,
          )
          .add_remote_process_plugin()
          .add_remote_wasm_plugin();
      })
      .write_file("/file.txt", "")
      .write_file("/test/exclude/other.txt", "")
      .write_file("/test/exclude/test-process-plugin-exact-file", "")
      .write_file("/test/exclude/test.txt_ps", "")
      .write_file("/test/test-process-plugin-exact-file", "")
      .initialize()
      .build();
    run_test_cli(vec!["output-file-paths", "**/*"], &environment).unwrap();
    let mut logged_messages = environment.take_stdout_messages();
    logged_messages.sort();
    assert_eq!(
      logged_messages,
      vec!["/file.txt", "/test/exclude/test.txt_ps", "/test/test-process-plugin-exact-file"]
    );
  }

  #[test]
  fn should_filter_by_cwd_in_sub_dir() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_includes("**/*.txt");
      })
      .write_file("/file.txt", "const t=4;")
      .write_file("/file2.txt", "const t=4;")
      .write_file("/sub/file3.txt", "const t=4;")
      .write_file("/sub2/file4.txt", "const t=4;")
      .set_cwd("/sub")
      .build();
    run_test_cli(vec!["output-file-paths"], &environment).unwrap();
    let mut logged_messages = environment.take_stdout_messages();
    logged_messages.sort();
    assert_eq!(logged_messages, vec!["/sub/file3.txt"]);
  }

  #[test]
  fn should_clear_cache_directory() {
    let environment = TestEnvironment::new();
    run_test_cli(vec!["clear-cache"], &environment).unwrap();
    assert_eq!(environment.take_stdout_messages(), vec!["Deleted /cache"]);
    assert_eq!(environment.is_dir_deleted("/cache"), true);
  }

  #[test]
  fn should_output_license_for_sub_command_with_no_plugins() {
    let environment = TestEnvironment::new();
    run_test_cli(vec!["license"], &environment).unwrap();
    assert_eq!(
      environment.take_stdout_messages(),
      vec!["==== DPRINT CLI LICENSE ====", std::str::from_utf8(include_bytes!("../../LICENSE")).unwrap()]
    );
  }

  #[test]
  fn should_output_license_for_sub_command_with_plugins() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin().build();
    run_test_cli(vec!["license"], &environment).unwrap();
    assert_eq!(
      environment.take_stdout_messages(),
      vec![
        "==== DPRINT CLI LICENSE ====",
        std::str::from_utf8(include_bytes!("../../LICENSE")).unwrap(),
        "\n==== TEST-PLUGIN LICENSE ====",
        r#"The MIT License (MIT)

Copyright (c) 2020-2023 David Sherret

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
"#,
        "\n==== TEST-PROCESS-PLUGIN LICENSE ====",
        "License text."
      ]
    );
  }

  #[test]
  fn should_output_shell_completions() {
    let environment = TestEnvironment::new();
    for kind in ["bash", "elvish", "fish", "powershell", "zsh"] {
      run_test_cli(vec!["completions", kind], &environment).unwrap();
      let logged_messages = environment.take_stdout_messages();
      assert_eq!(logged_messages.len(), 1);
      assert!(!logged_messages[0].contains("hidden"));
    }
  }
}
