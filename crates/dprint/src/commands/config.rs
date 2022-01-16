use anyhow::anyhow;
use anyhow::bail;
use anyhow::Error;
use anyhow::Result;
use std::collections::HashSet;
use std::path::PathBuf;
use url::Url;

use crate::arg_parser::CliArgs;
use crate::cache::Cache;
use crate::configuration::get_init_config_file_text;
use crate::configuration::*;
use crate::environment::Environment;
use crate::plugins::output_plugin_config_diagnostics;
use crate::plugins::read_info_file;
use crate::plugins::resolve_plugins;
use crate::plugins::InfoFilePluginInfo;
use crate::plugins::Plugin;
use crate::plugins::PluginResolver;
use crate::plugins::PluginSourceReference;
use crate::utils::pretty_print_json_text;
use crate::utils::ErrorCountLogger;
use crate::utils::PathSource;

pub fn init_config_file(environment: &impl Environment, config_arg: &Option<String>) -> Result<()> {
  let config_file_path = get_config_path(config_arg)?;
  return if !environment.path_exists(&config_file_path) {
    environment.write_file(&config_file_path, &get_init_config_file_text(environment)?)?;
    environment.log_stderr(&format!("\nCreated {}", config_file_path.display()));
    environment.log_stderr("\nIf you are working in a commercial environment please consider sponsoring dprint: https://dprint.dev/sponsor");
    Ok(())
  } else {
    bail!("Configuration file '{}' already exists.", config_file_path.display())
  };

  fn get_config_path(config_arg: &Option<String>) -> Result<PathBuf> {
    return Ok(if let Some(config_arg) = config_arg.as_ref() {
      PathBuf::from(config_arg)
    } else {
      PathBuf::from("./dprint.json")
    });
  }
}

pub fn add_plugin_config_file<TEnvironment: Environment>(
  args: &CliArgs,
  plugin_name_or_url: Option<&String>,
  cache: &Cache<TEnvironment>,
  environment: &TEnvironment,
  plugin_resolver: &PluginResolver<TEnvironment>,
) -> Result<()> {
  let config = resolve_config_from_args(args, cache, environment)?;
  let config_path = match config.resolved_path.source {
    PathSource::Local(source) => source.path,
    PathSource::Remote(_) => bail!("Cannot update plugins in a remote configuration."),
  };
  let plugin_url_to_add = match plugin_name_or_url {
    Some(plugin_name_or_url) => match Url::parse(plugin_name_or_url) {
      Ok(url) => url.to_string(),
      Err(_) => {
        let info_file = read_info_file(environment).map_err(|err| anyhow!("Error downloading info file. {}", err))?;
        let plugin = info_file
          .latest_plugins
          .into_iter()
          .find(|plugin| &plugin.name == plugin_name_or_url)
          .ok_or_else(|| anyhow!("Could not find plugin with name '{}'. Please specify a url instead.", plugin_name_or_url))?;
        for (config_plugin_reference, current_plugin) in get_config_file_plugins(plugin_resolver, config.plugins) {
          if let Ok(current_plugin) = current_plugin {
            if current_plugin.name() == plugin.name {
              if current_plugin.version() != plugin.version {
                let file_text = environment.read_file(&config_path)?;
                let file_text = update_plugin_in_config(
                  &file_text,
                  PluginUpdateInfo {
                    name: current_plugin.name().to_string(),
                    old_version: current_plugin.version().to_string(),
                    old_reference: config_plugin_reference,
                    new_plugin: plugin,
                  },
                );
                environment.write_file(&config_path, &file_text)?;
              }
              return Ok(());
            }
          }
        }
        plugin.full_url_no_wasm_checksum()
      }
    },
    None => {
      let mut possible_plugins = get_possible_plugins_to_add(environment, plugin_resolver, config.plugins)?;
      if possible_plugins.is_empty() {
        bail!("Could not find any plugins to add. Please provide one by specifying `dprint config add <plugin-url>`.");
      }
      let index = environment.get_selection(
        "Select a plugin to add:",
        0,
        &possible_plugins.iter().map(|p| p.name.clone()).collect::<Vec<_>>(),
      )?;
      possible_plugins.remove(index).full_url_no_wasm_checksum()
    }
  };

  let file_text = environment.read_file(&config_path)?;
  let new_text = add_to_plugins_array(&file_text, &plugin_url_to_add)?;
  environment.write_file(&config_path, &new_text)?;

  Ok(())
}

fn get_possible_plugins_to_add<TEnvironment: Environment>(
  environment: &TEnvironment,
  plugin_resolver: &PluginResolver<TEnvironment>,
  current_plugins: Vec<PluginSourceReference>,
) -> Result<Vec<InfoFilePluginInfo>> {
  let info_file = read_info_file(environment).map_err(|err| anyhow!("Error downloading info file. {}", err))?;
  let current_plugin_names = get_config_file_plugins(plugin_resolver, current_plugins)
    .into_iter()
    .filter_map(|(plugin_reference, plugin_result)| match plugin_result {
      Ok(plugin) => Some(plugin.name().to_string()),
      Err(error) => {
        environment.log_stderr(&format!("Error resolving plugin: {}\n\n{}", plugin_reference.path_source.display(), error));
        None
      }
    })
    .collect::<HashSet<_>>();
  Ok(
    info_file
      .latest_plugins
      .into_iter()
      .filter(|p| !current_plugin_names.contains(&p.name))
      .collect(),
  )
}

pub fn update_plugins_config_file<TEnvironment: Environment>(
  args: &CliArgs,
  cache: &Cache<TEnvironment>,
  environment: &TEnvironment,
  plugin_resolver: &PluginResolver<TEnvironment>,
) -> Result<()> {
  let config = resolve_config_from_args(args, cache, environment)?;
  let config_path = match config.resolved_path.source {
    PathSource::Local(source) => source.path,
    PathSource::Remote(_) => bail!("Cannot update plugins in a remote configuration."),
  };
  let mut file_text = environment.read_file(&config_path)?;
  let plugins_to_update = get_plugins_to_update(environment, plugin_resolver, config.plugins)?;

  for result in plugins_to_update {
    match result {
      Ok(info) => {
        let should_update = if info.is_wasm() {
          true
        } else {
          // prompt for security reasons
          environment.log_stderr(&format!(
            "The process plugin {} {} has a new url: {}",
            info.name,
            info.old_version,
            info.get_full_new_config_url(),
          ));
          environment.confirm("Do you wish to update it?", false)?
        };

        if should_update {
          environment.log_stderr(&format!("Updating {} {} to {}...", info.name, info.old_version, info.new_plugin.version));
          file_text = update_plugin_in_config(&file_text, info);
        }
      }
      Err(err_info) => {
        environment.log_stderr(&format!("Error updating plugin {}: {}", err_info.name, err_info.error));
      }
    }
  }

  environment.write_file(&config_path, &file_text)?;

  Ok(())
}

struct PluginUpdateError {
  name: String,
  error: Error,
}

fn get_plugins_to_update<TEnvironment: Environment>(
  environment: &TEnvironment,
  plugin_resolver: &PluginResolver<TEnvironment>,
  plugins: Vec<PluginSourceReference>,
) -> Result<Vec<Result<PluginUpdateInfo, PluginUpdateError>>> {
  let info_file = read_info_file(environment).map_err(|err| anyhow!("Error downloading info file. {}", err))?;
  let config_file_plugins = get_config_file_plugins(plugin_resolver, plugins);
  Ok(
    config_file_plugins
      .into_iter()
      .filter_map(|(plugin_reference, plugin_result)| {
        let plugin = match plugin_result {
          Ok(plugin) => plugin,
          Err(error) => {
            return Some(Err(PluginUpdateError {
              name: plugin_reference.path_source.display(),
              error,
            }))
          }
        };
        let latest_plugin_info = info_file.latest_plugins.iter().find(|p| p.name == plugin.name());
        let latest_plugin_info = match latest_plugin_info {
          Some(i) => i,
          None => return None,
        };
        if plugin.version() == latest_plugin_info.version {
          return None;
        }

        Some(Ok(PluginUpdateInfo {
          name: plugin.name().to_string(),
          old_reference: plugin_reference,
          old_version: plugin.version().to_string(),
          new_plugin: latest_plugin_info.clone(),
        }))
      })
      .collect::<Vec<_>>(),
  )
}

pub fn output_resolved_config<TEnvironment: Environment>(
  args: &CliArgs,
  cache: &Cache<TEnvironment>,
  environment: &TEnvironment,
  plugin_resolver: &PluginResolver<TEnvironment>,
) -> Result<()> {
  let config = resolve_config_from_args(args, cache, environment)?;
  let plugins = resolve_plugins(args, &config, environment, plugin_resolver)?;

  let mut plugin_jsons = Vec::new();
  for plugin in plugins {
    let config_key = String::from(plugin.config_key());

    // get an initialized plugin and output its diagnostics
    let initialized_plugin = plugin.initialize()?;
    output_plugin_config_diagnostics(plugin.name(), &*initialized_plugin, &ErrorCountLogger::from_environment(environment))?;

    let text = initialized_plugin.get_resolved_config()?;
    let pretty_text = pretty_print_json_text(&text)?;
    plugin_jsons.push(format!("\"{}\": {}", config_key, pretty_text));
  }

  if plugin_jsons.is_empty() {
    environment.log("{}");
  } else {
    let text = plugin_jsons.join(",\n").lines().map(|l| format!("  {}", l)).collect::<Vec<_>>().join("\n");
    environment.log(&format!("{{\n{}\n}}", text));
  }

  Ok(())
}

fn get_config_file_plugins<TEnvironment: Environment>(
  plugin_resolver: &PluginResolver<TEnvironment>,
  current_plugins: Vec<PluginSourceReference>,
) -> Vec<(PluginSourceReference, Result<Box<dyn Plugin>>)> {
  use rayon::iter::IntoParallelIterator;
  use rayon::iter::ParallelIterator;

  current_plugins
    .into_par_iter()
    .map(|plugin_reference| {
      let resolve_result = plugin_resolver.resolve_plugin(&plugin_reference);
      (plugin_reference, resolve_result)
    })
    .collect::<Vec<_>>()
}

#[cfg(test)]
mod test {
  use anyhow::Result;
  use pretty_assertions::assert_eq;

  use crate::configuration::*;
  use crate::environment::Environment;
  use crate::environment::TestEnvironment;
  use crate::environment::TestEnvironmentBuilder;
  use crate::environment::TestInfoFilePlugin;
  use crate::test_helpers;
  use crate::test_helpers::run_test_cli;

  #[test]
  fn should_initialize() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info
          .add_plugin(TestInfoFilePlugin {
            name: "dprint-plugin-typescript".to_string(),
            version: "0.17.2".to_string(),
            url: "https://plugins.dprint.dev/typescript-0.17.2.wasm".to_string(),
            config_key: Some("typescript".to_string()),
            file_extensions: vec!["ts".to_string()],
            config_excludes: vec![],
            ..Default::default()
          })
          .add_plugin(TestInfoFilePlugin {
            name: "dprint-plugin-jsonc".to_string(),
            version: "0.2.3".to_string(),
            url: "https://plugins.dprint.dev/json-0.2.3.wasm".to_string(),
            config_key: Some("json".to_string()),
            file_extensions: vec!["json".to_string()],
            config_excludes: vec![],
            ..Default::default()
          });
      })
      .build();
    let expected_text = get_init_config_file_text(&environment).unwrap();
    environment.clear_logs();
    run_test_cli(vec!["init"], &environment).unwrap();
    assert_eq!(
      environment.take_stderr_messages(),
      vec![
        "Select plugins (use the spacebar to select/deselect and then press enter when finished):",
        "\nCreated ./dprint.json",
        "\nIf you are working in a commercial environment please consider sponsoring dprint: https://dprint.dev/sponsor"
      ]
    );
    assert_eq!(environment.read_file("./dprint.json").unwrap(), expected_text);
  }

  #[test]
  fn should_use_dprint_config_init_as_alias() {
    let environment = TestEnvironment::new();
    let expected_text = get_init_config_file_text(&environment).unwrap();
    environment.clear_logs();
    run_test_cli(vec!["config", "init"], &environment).unwrap();
    environment.take_stderr_messages();
    environment.take_stdout_messages();
    assert_eq!(environment.read_file("./dprint.json").unwrap(), expected_text);
  }

  #[test]
  fn should_initialize_with_specified_config_path() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info.add_plugin(TestInfoFilePlugin {
          name: "dprint-plugin-typescript".to_string(),
          version: "0.17.2".to_string(),
          url: "https://plugins.dprint.dev/typescript-0.17.2.wasm".to_string(),
          config_key: Some("typescript".to_string()),
          file_extensions: vec!["ts".to_string()],
          config_excludes: vec![],
          ..Default::default()
        });
      })
      .build();
    let expected_text = get_init_config_file_text(&environment).unwrap();
    environment.clear_logs();
    run_test_cli(vec!["init", "--config", "./test.config.json"], &environment).unwrap();
    assert_eq!(
      environment.take_stderr_messages(),
      vec![
        "Select plugins (use the spacebar to select/deselect and then press enter when finished):",
        "\nCreated ./test.config.json",
        "\nIf you are working in a commercial environment please consider sponsoring dprint: https://dprint.dev/sponsor"
      ]
    );
    assert_eq!(environment.read_file("./test.config.json").unwrap(), expected_text);
  }

  #[test]
  fn should_error_when_config_file_exists_on_initialize() {
    let environment = TestEnvironmentBuilder::new()
      .with_default_config(|c| {
        c.add_includes("**/*.txt");
      })
      .build();
    let error_message = run_test_cli(vec!["init"], &environment).err().unwrap();
    assert_eq!(error_message.to_string(), "Configuration file './dprint.json' already exists.");
  }

  #[test]
  fn config_add() {
    let old_wasm_url = "https://plugins.dprint.dev/test-plugin.wasm".to_string();
    let new_wasm_url = "https://plugins.dprint.dev/test-plugin-2.wasm".to_string();
    let old_ps_checksum = test_helpers::get_test_process_plugin_checksum();
    let old_ps_url = format!("https://plugins.dprint.dev/test-process.exe-plugin@{}", old_ps_checksum);
    let new_ps_url = "https://plugins.dprint.dev/test-plugin-3.exe-plugin".to_string();
    let new_ps_url_with_checksum = format!("{}@{}", new_ps_url, "info-checksum");
    let select_plugin_msg = "Select a plugin to add:".to_string();

    // no plugins specified
    test_add(TestAddOptions {
      add_arg: None,
      config_has_wasm: false,
      config_has_process: false,
      info_has_checksum: false,
      expected_error: None,
      expected_logs: vec![select_plugin_msg.clone()],
      expected_urls: vec![new_wasm_url.clone()],
      selection_result: Some(0),
    });

    // process plugin specified
    test_add(TestAddOptions {
      add_arg: None,
      config_has_wasm: false,
      config_has_process: false,
      info_has_checksum: true,
      expected_error: None,
      expected_logs: vec![select_plugin_msg.clone()],
      expected_urls: vec![new_ps_url_with_checksum.clone()],
      selection_result: Some(1),
    });

    // process plugin specified no checksum in info
    test_add(TestAddOptions {
      add_arg: None,
      config_has_wasm: false,
      config_has_process: false,
      info_has_checksum: false,
      expected_error: None,
      expected_logs: vec![select_plugin_msg.clone()],
      expected_urls: vec![new_ps_url.clone()],
      selection_result: Some(1),
    });

    // wasm exists, no process
    test_add(TestAddOptions {
      add_arg: None,
      config_has_wasm: true,
      config_has_process: false,
      info_has_checksum: false,
      expected_error: None,
      expected_logs: vec![select_plugin_msg.clone()],
      expected_urls: vec![old_wasm_url.clone(), new_ps_url.clone()],
      selection_result: Some(0),
    });

    // process exists, no wasm
    test_add(TestAddOptions {
      add_arg: None,
      config_has_wasm: false,
      config_has_process: true,
      info_has_checksum: false,
      expected_error: None,
      expected_logs: vec![select_plugin_msg.clone()],
      expected_urls: vec![old_ps_url.clone(), new_wasm_url.clone()],
      selection_result: Some(0),
    });

    // all plugins already specified in config
    test_add(TestAddOptions {
      add_arg: None,
      config_has_wasm: true,
      config_has_process: true,
      info_has_checksum: false,
      expected_error: Some("Could not find any plugins to add. Please provide one by specifying `dprint config add <plugin-url>`."),
      expected_logs: vec![],
      expected_urls: vec![],
      selection_result: Some(0),
    });

    // using arg
    test_add(TestAddOptions {
      add_arg: Some("test-plugin"),
      config_has_wasm: false,
      config_has_process: false,
      info_has_checksum: false,
      expected_error: None,
      expected_logs: vec![],
      expected_urls: vec![new_wasm_url.clone()],
      selection_result: Some(0),
    });

    // using and already exists
    test_add(TestAddOptions {
      add_arg: Some("test-plugin"),
      config_has_wasm: true,
      config_has_process: false,
      info_has_checksum: false,
      expected_error: None,
      expected_logs: vec![],
      expected_urls: vec![
        // upgrades to the latest
        new_wasm_url,
      ],
      selection_result: Some(0),
    });

    // using url
    test_add(TestAddOptions {
      add_arg: Some("https://plugins.dprint.dev/my-plugin.wasm"),
      config_has_wasm: false,
      config_has_process: false,
      info_has_checksum: false,
      expected_error: None,
      expected_logs: vec![],
      expected_urls: vec!["https://plugins.dprint.dev/my-plugin.wasm".to_string()],
      selection_result: Some(0),
    });
  }

  #[derive(Debug)]
  struct TestAddOptions {
    add_arg: Option<&'static str>,
    config_has_wasm: bool,
    config_has_process: bool,
    info_has_checksum: bool,
    selection_result: Option<usize>,
    expected_error: Option<&'static str>,
    expected_logs: Vec<String>,
    expected_urls: Vec<String>,
  }

  fn test_add(options: TestAddOptions) {
    let expected_logs = options.expected_logs.clone();
    let expected_urls = options.expected_urls.clone();
    let environment = get_setup_env(SetupEnvOptions {
      config_has_wasm: options.config_has_wasm,
      config_has_wasm_checksum: false,
      config_has_process: options.config_has_process,
      info_has_checksum: options.info_has_checksum,
    });
    if let Some(selection_result) = options.selection_result {
      environment.set_selection_result(selection_result);
    }
    let mut args = vec!["config", "add"];
    if let Some(add_arg) = options.add_arg {
      args.push(add_arg);
    }
    match run_test_cli(args, &environment) {
      Ok(()) => {
        assert!(options.expected_error.is_none());
      }
      Err(err) => {
        assert_eq!(Some(err.to_string()), options.expected_error.map(ToOwned::to_owned));
      }
    }
    assert_eq!(environment.take_stderr_messages(), expected_logs);

    if options.expected_error.is_none() {
      let expected_text = format!(
        r#"{{
  "plugins": [
{}
  ]
}}"#,
        expected_urls.into_iter().map(|u| format!("    \"{}\"", u)).collect::<Vec<_>>().join(",\n")
      );
      assert_eq!(environment.read_file("./dprint.json").unwrap(), expected_text);
    }
  }

  #[test]
  fn config_update_should_upgrade_to_latest_plugins() {
    let new_wasm_url = "https://plugins.dprint.dev/test-plugin-2.wasm".to_string();
    let new_wasm_url_with_checksum = format!("{}@{}", new_wasm_url, "info-checksum");
    let updating_message = "Updating test-plugin 0.1.0 to 0.2.0...".to_string();

    // test all the wasm combinations
    test_update(TestUpdateOptions {
      config_has_wasm: true,
      config_has_wasm_checksum: true,
      config_has_process: false,
      info_has_checksum: true,
      confirm_results: Vec::new(),
      expected_logs: vec![updating_message.clone()],
      expected_urls: vec![new_wasm_url_with_checksum.clone()],
    });
    test_update(TestUpdateOptions {
      config_has_wasm: true,
      config_has_wasm_checksum: true,
      config_has_process: false,
      info_has_checksum: false,
      confirm_results: Vec::new(),
      expected_logs: vec![updating_message.clone()],
      expected_urls: vec![new_wasm_url.clone()],
    });
    test_update(TestUpdateOptions {
      config_has_wasm: true,
      config_has_wasm_checksum: false,
      config_has_process: false,
      info_has_checksum: true,
      confirm_results: Vec::new(),
      expected_logs: vec![updating_message.clone()],
      expected_urls: vec![new_wasm_url.clone()],
    });
    test_update(TestUpdateOptions {
      config_has_wasm: true,
      config_has_wasm_checksum: false,
      config_has_process: false,
      info_has_checksum: false,
      confirm_results: Vec::new(),
      expected_logs: vec![updating_message.clone()],
      expected_urls: vec![new_wasm_url.clone()],
    });

    // test all the process plugin combinations
    let old_ps_checksum = test_helpers::get_test_process_plugin_checksum();
    let old_ps_url = format!("https://plugins.dprint.dev/test-process.exe-plugin@{}", old_ps_checksum);
    let new_ps_url = "https://plugins.dprint.dev/test-plugin-3.exe-plugin".to_string();
    let new_ps_url_with_checksum = format!("{}@{}", new_ps_url, "info-checksum");
    test_update(TestUpdateOptions {
      config_has_wasm: false,
      config_has_wasm_checksum: false,
      config_has_process: true,
      info_has_checksum: true,
      confirm_results: vec![Ok(Some(true))],
      expected_logs: vec![
        format!("The process plugin test-process-plugin 0.1.0 has a new url: {}", new_ps_url_with_checksum),
        "Do you wish to update it? Y".to_string(),
        "Updating test-process-plugin 0.1.0 to 0.3.0...".to_string(),
      ],
      expected_urls: vec![new_ps_url_with_checksum.clone()],
    });
    test_update(TestUpdateOptions {
      config_has_wasm: false,
      config_has_wasm_checksum: false,
      config_has_process: true,
      info_has_checksum: false,
      confirm_results: vec![Ok(Some(true))],
      expected_logs: vec![
        format!("The process plugin test-process-plugin 0.1.0 has a new url: {}", new_ps_url),
        "Do you wish to update it? Y".to_string(),
        "Updating test-process-plugin 0.1.0 to 0.3.0...".to_string(),
      ],
      expected_urls: vec![new_ps_url.clone()],
    });
    test_update(TestUpdateOptions {
      config_has_wasm: false,
      config_has_wasm_checksum: false,
      config_has_process: true,
      info_has_checksum: false,
      confirm_results: vec![Ok(None)],
      expected_logs: vec![
        format!("The process plugin test-process-plugin 0.1.0 has a new url: {}", new_ps_url),
        "Do you wish to update it? N".to_string(),
      ],
      expected_urls: vec![old_ps_url.clone()],
    });

    // testing both in config, but only updating one
    test_update(TestUpdateOptions {
      config_has_wasm: true,
      config_has_wasm_checksum: false,
      config_has_process: true,
      info_has_checksum: false,
      confirm_results: vec![Ok(Some(false))],
      expected_logs: vec![
        "Updating test-plugin 0.1.0 to 0.2.0...".to_string(),
        format!("The process plugin test-process-plugin 0.1.0 has a new url: {}", new_ps_url),
        "Do you wish to update it? N".to_string(),
      ],
      expected_urls: vec![new_wasm_url.clone(), old_ps_url.clone()],
    });
  }

  #[derive(Debug)]
  struct TestUpdateOptions {
    config_has_wasm: bool,
    config_has_wasm_checksum: bool,
    config_has_process: bool,
    info_has_checksum: bool,
    confirm_results: Vec<Result<Option<bool>>>,
    expected_logs: Vec<String>,
    expected_urls: Vec<String>,
  }

  fn test_update(options: TestUpdateOptions) {
    let expected_logs = options.expected_logs.clone();
    let expected_urls = options.expected_urls.clone();
    let environment = get_setup_env(SetupEnvOptions {
      config_has_wasm: options.config_has_wasm,
      config_has_wasm_checksum: options.config_has_wasm_checksum,
      config_has_process: options.config_has_process,
      info_has_checksum: options.info_has_checksum,
    });
    environment.set_confirm_results(options.confirm_results);
    run_test_cli(vec!["config", "update"], &environment).unwrap();
    assert_eq!(environment.take_stderr_messages(), expected_logs);

    let expected_text = format!(
      r#"{{
  "plugins": [
{}
  ]
}}"#,
      expected_urls.into_iter().map(|u| format!("    \"{}\"", u)).collect::<Vec<_>>().join(",\n")
    );
    assert_eq!(environment.read_file("./dprint.json").unwrap(), expected_text);
  }

  #[derive(Debug)]
  struct SetupEnvOptions {
    config_has_wasm: bool,
    config_has_wasm_checksum: bool,
    config_has_process: bool,
    info_has_checksum: bool,
  }

  fn get_setup_env(opts: SetupEnvOptions) -> TestEnvironment {
    let actual_wasm_plugin_checksum = test_helpers::get_test_wasm_plugin_checksum();
    let mut builder = TestEnvironmentBuilder::new();

    if opts.config_has_wasm {
      builder.add_remote_wasm_plugin();
    }
    if opts.config_has_process {
      builder.add_remote_process_plugin();
    }

    builder
      .with_info_file(|info| {
        info.add_plugin(TestInfoFilePlugin {
          name: "test-plugin".to_string(),
          version: "0.2.0".to_string(),
          url: "https://plugins.dprint.dev/test-plugin-2.wasm".to_string(),
          config_key: Some("typescript".to_string()),
          checksum: if opts.info_has_checksum { Some("info-checksum".to_string()) } else { None },
          ..Default::default()
        });

        info.add_plugin(TestInfoFilePlugin {
          name: "test-process-plugin".to_string(),
          version: "0.3.0".to_string(),
          url: "https://plugins.dprint.dev/test-plugin-3.exe-plugin".to_string(),
          config_key: Some("typescript".to_string()),
          checksum: if opts.info_has_checksum { Some("info-checksum".to_string()) } else { None },
          ..Default::default()
        });
      })
      .with_default_config(|config| {
        config.ensure_plugins_section();
        if opts.config_has_wasm {
          if opts.config_has_wasm_checksum {
            config.add_remote_wasm_plugin_with_checksum(&actual_wasm_plugin_checksum);
          } else {
            config.add_remote_wasm_plugin();
          }
        }
        if opts.config_has_process {
          // this will add it with the checksum
          // Don't bother testing this without a checksum because it won't resolve the plugin
          config.add_remote_process_plugin();
        }
      });

    builder.initialize().build()
  }

  #[test]
  fn config_update_should_not_upgrade_when_at_latest_plugins() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_info_file(|info| {
        info.add_plugin(TestInfoFilePlugin {
          name: "test-plugin".to_string(),
          version: "0.1.0".to_string(),
          url: "https://plugins.dprint.dev/test-plugin.wasm".to_string(),
          config_key: Some("plugin".to_string()),
          checksum: None,
          ..Default::default()
        });
      })
      .with_default_config(|config| {
        config.add_remote_wasm_plugin();
      })
      .initialize()
      .build();
    run_test_cli(vec!["config", "update"], &environment).unwrap();
    // should be empty because nothing to upgrade
    assert!(environment.take_stderr_messages().is_empty());
  }

  #[test]
  fn config_update_should_handle_wasm_to_process_plugin() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_info_file(|info| {
        info.add_plugin(TestInfoFilePlugin {
          name: "test-plugin".to_string(),
          version: "0.2.0".to_string(),
          url: "https://plugins.dprint.dev/test-plugin.exe-plugin".to_string(),
          config_key: Some("plugin".to_string()),
          checksum: Some("checksum".to_string()),
          ..Default::default()
        });
      })
      .with_default_config(|config| {
        config.add_remote_wasm_plugin();
      })
      .initialize()
      .build();
    environment.set_confirm_results(vec![Ok(None)]);
    run_test_cli(vec!["config", "update"], &environment).unwrap();
    assert_eq!(
      environment.take_stderr_messages(),
      vec![
        "The process plugin test-plugin 0.1.0 has a new url: https://plugins.dprint.dev/test-plugin.exe-plugin@checksum",
        "Do you wish to update it? N"
      ]
    );
  }

  #[test]
  fn should_output_resolved_config() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin().build();
    run_test_cli(vec!["output-resolved-config"], &environment).unwrap();
    assert_eq!(
      environment.take_stdout_messages(),
      vec![concat!(
        "{\n",
        "  \"test-plugin\": {\n",
        "    \"ending\": \"formatted\",\n",
        "    \"lineWidth\": 120\n",
        "  },\n",
        "  \"testProcessPlugin\": {\n",
        "    \"ending\": \"formatted_process\",\n",
        "    \"lineWidth\": 120\n",
        "  }\n",
        "}",
      )]
    );
  }

  #[test]
  fn should_output_resolved_config_no_plugins() {
    let environment = TestEnvironmentBuilder::new().with_default_config(|_| {}).build();
    run_test_cli(vec!["output-resolved-config"], &environment).unwrap();
    assert_eq!(environment.take_stdout_messages(), vec!["{}"]);
  }
}
