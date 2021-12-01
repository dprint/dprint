use dprint_cli_core::types::ErrBox;

use crate::cache::Cache;
use crate::cli::configuration::resolve_config_from_args;
use crate::cli::paths::get_and_resolve_file_paths;
use crate::cli::paths::get_file_paths_by_plugin;
use crate::cli::plugins::get_plugins_from_args;
use crate::cli::plugins::resolve_plugins_and_err_if_empty;
use crate::cli::CliArgs;
use crate::environment::Environment;
use crate::plugins::PluginResolver;
use crate::utils::get_table_text;

pub fn output_version<'a, TEnvironment: Environment>(environment: &TEnvironment) -> Result<(), ErrBox> {
  environment.log(&format!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")));

  Ok(())
}

pub fn output_help<TEnvironment: Environment>(
  args: &CliArgs,
  cache: &Cache<TEnvironment>,
  environment: &TEnvironment,
  plugin_resolver: &PluginResolver<TEnvironment>,
  help_text: &str,
) -> Result<(), ErrBox> {
  // log the cli's help first
  environment.log(help_text);

  // now check for the plugins
  let plugins_result = get_plugins_from_args(args, cache, environment, plugin_resolver);
  match plugins_result {
    Ok(plugins) => {
      if !plugins.is_empty() {
        let table_text = get_table_text(plugins.iter().map(|plugin| (plugin.name(), plugin.help_url())).collect());
        environment.log("\nPLUGINS HELP:");
        environment.log(&table_text.render(
          4, // indent
          // don't render taking terminal width into account
          // as these are urls and we want them to be clickable
          None,
        ));
      }
    }
    Err(err) => {
      log_verbose!(environment, "Error getting plugins for help. {}", err.to_string());
    }
  }

  Ok(())
}

pub fn output_license<TEnvironment: Environment>(
  args: &CliArgs,
  cache: &Cache<TEnvironment>,
  environment: &TEnvironment,
  plugin_resolver: &PluginResolver<TEnvironment>,
) -> Result<(), ErrBox> {
  environment.log("==== DPRINT CLI LICENSE ====");
  environment.log(std::str::from_utf8(include_bytes!("../../../LICENSE"))?);

  // now check for the plugins
  for plugin in get_plugins_from_args(args, cache, environment, plugin_resolver)? {
    environment.log(&format!("\n==== {} LICENSE ====", plugin.name().to_uppercase()));
    let initialized_plugin = plugin.initialize()?;
    environment.log(&initialized_plugin.get_license_text()?);
  }

  Ok(())
}

pub fn clear_cache(environment: &impl Environment) -> Result<(), ErrBox> {
  let cache_dir = environment.get_cache_dir();
  environment.remove_dir_all(&cache_dir)?;
  environment.log_stderr(&format!("Deleted {}", cache_dir.display()));
  Ok(())
}

pub fn output_file_paths<TEnvironment: Environment>(
  args: &CliArgs,
  environment: &TEnvironment,
  cache: &Cache<TEnvironment>,
  plugin_resolver: &PluginResolver<TEnvironment>,
) -> Result<(), ErrBox> {
  let config = resolve_config_from_args(args, cache, environment)?;
  let plugins = resolve_plugins_and_err_if_empty(args, &config, environment, plugin_resolver)?;
  let resolved_file_paths = get_and_resolve_file_paths(&config, args, environment)?;
  let file_paths_by_plugin = get_file_paths_by_plugin(&plugins, resolved_file_paths)?;

  let file_paths = file_paths_by_plugin.values().flat_map(|x| x.iter());
  for file_path in file_paths {
    environment.log(&file_path.display().to_string())
  }
  Ok(())
}

#[cfg(test)]
mod test {
  use pretty_assertions::assert_eq;

  use crate::environment::TestEnvironment;
  use crate::environment::TestEnvironmentBuilder;
  use crate::test_helpers::get_expected_help_text;
  use crate::test_helpers::run_test_cli;

  #[test]
  fn it_should_output_version_with_v() {
    let environment = TestEnvironment::new();
    run_test_cli(vec!["-v"], &environment).unwrap();
    let logged_messages = environment.take_stdout_messages();
    assert_eq!(logged_messages, vec![format!("dprint {}", env!("CARGO_PKG_VERSION"))]);
  }

  #[test]
  fn it_should_output_version_with_no_plugins() {
    let environment = TestEnvironment::new();
    run_test_cli(vec!["--version"], &environment).unwrap();
    let logged_messages = environment.take_stdout_messages();
    assert_eq!(logged_messages, vec![format!("dprint {}", env!("CARGO_PKG_VERSION"))]);
  }

  #[test]
  fn it_should_output_version_and_ignore_plugins() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin().build();
    run_test_cli(vec!["--version"], &environment).unwrap();
    let logged_messages = environment.take_stdout_messages();
    assert_eq!(logged_messages, vec![format!("dprint {}", env!("CARGO_PKG_VERSION"))]);
  }

  #[test]
  fn it_should_output_help_with_no_plugins() {
    let environment = TestEnvironment::new();
    run_test_cli(vec!["--help"], &environment).unwrap();
    let logged_messages = environment.take_stdout_messages();
    assert_eq!(logged_messages, vec![get_expected_help_text()]);
  }

  #[test]
  fn it_should_output_help_no_sub_commands() {
    let environment = TestEnvironment::new();
    run_test_cli(vec![], &environment).unwrap();
    let logged_messages = environment.take_stdout_messages();
    assert_eq!(logged_messages, vec![get_expected_help_text()]);
  }

  #[test]
  fn it_should_output_help_with_plugins() {
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
  fn it_should_output_resolved_file_paths() {
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
  fn it_should_not_output_file_paths_not_supported_by_plugins() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin()
      .write_file("/file.ts", "const t=4;")
      .write_file("/file2.ts", "const t=4;")
      .build();
    run_test_cli(vec!["output-file-paths", "**/*.*"], &environment).unwrap();
    assert_eq!(environment.take_stdout_messages().len(), 0);
  }

  #[test]
  fn it_should_output_resolved_file_paths_when_using_backslashes() {
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
  fn it_should_filter_by_cwd_in_sub_dir() {
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
  fn it_should_clear_cache_directory() {
    let environment = TestEnvironment::new();
    run_test_cli(vec!["clear-cache"], &environment).unwrap();
    assert_eq!(environment.take_stderr_messages(), vec!["Deleted /cache"]);
    assert_eq!(environment.is_dir_deleted("/cache"), true);
  }
  #[test]
  fn it_should_output_license_for_sub_command_with_no_plugins() {
    let environment = TestEnvironment::new();
    run_test_cli(vec!["license"], &environment).unwrap();
    assert_eq!(
      environment.take_stdout_messages(),
      vec!["==== DPRINT CLI LICENSE ====", std::str::from_utf8(include_bytes!("../../../LICENSE")).unwrap()]
    );
  }

  #[test]
  fn it_should_output_license_for_sub_command_with_plugins() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin().build();
    run_test_cli(vec!["license"], &environment).unwrap();
    assert_eq!(
      environment.take_stdout_messages(),
      vec![
        "==== DPRINT CLI LICENSE ====",
        std::str::from_utf8(include_bytes!("../../../LICENSE")).unwrap(),
        "\n==== TEST-PLUGIN LICENSE ====",
        r#"The MIT License (MIT)

Copyright (c) 2020 David Sherret

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
}
