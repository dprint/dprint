use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use dprint_core::async_runtime::future;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;
use url::Url;

use crate::arg_parser::CliArgs;
use crate::arg_parser::FilePatternArgs;
use crate::configuration::get_init_config_file_text;
use crate::configuration::*;
use crate::environment::Environment;
use crate::plugins::read_info_file;
use crate::plugins::read_update_url;
use crate::plugins::InfoFilePluginInfo;
use crate::plugins::PluginResolver;
use crate::plugins::PluginSourceReference;
use crate::plugins::PluginWrapper;
use crate::resolution::resolve_plugins_scope;
use crate::resolution::resolve_plugins_scope_and_paths;
use crate::resolution::GetPluginResult;
use crate::utils::pretty_print_json_text;
use crate::utils::CachedDownloader;
use crate::utils::PathSource;

pub async fn init_config_file(environment: &impl Environment, config_arg: &Option<String>) -> Result<()> {
  let config_file_path = get_config_path(config_arg)?;
  return if !environment.path_exists(&config_file_path) {
    environment.write_file(&config_file_path, &get_init_config_file_text(environment).await?)?;
    log_stdout_info!(environment, "\nCreated {}", config_file_path.display());
    log_stdout_info!(
      environment,
      "\nIf you are working in a commercial environment please consider sponsoring dprint: https://dprint.dev/sponsor"
    );
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

pub async fn add_plugin_config_file<TEnvironment: Environment>(
  args: &CliArgs,
  plugin_name_or_url: Option<&String>,
  environment: &TEnvironment,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
) -> Result<()> {
  let config = resolve_config_from_args(args, environment).await?;
  let config_path = match config.resolved_path.source {
    PathSource::Local(source) => source.path,
    PathSource::Remote(_) => bail!("Cannot update plugins in a remote configuration."),
  };
  let plugin_url_to_add = match plugin_name_or_url {
    Some(plugin_name_or_url) => match Url::parse(plugin_name_or_url) {
      Ok(url) => url.to_string(),
      Err(_) => {
        let cached_downloader = CachedDownloader::new(environment.clone());
        let plugin_name = if plugin_name_or_url.contains('/') {
          plugin_name_or_url.to_string()
        } else {
          format!("dprint/{}", plugin_name_or_url)
        };
        let plugin = match read_update_url(&cached_downloader, &format!("https://plugins.dprint.dev/{}/latest.json", plugin_name)).await? {
          Some(result) => result,
          None => {
            let trailing_message = if let Ok(possible_plugins) = get_possible_plugins_to_add(environment, plugin_resolver, config.plugins).await {
              if possible_plugins.is_empty() {
                String::new()
              } else {
                format!(
                  "\n\nPlugins:\n{}",
                  possible_plugins.iter().map(|p| format!(" * {}", p.name)).collect::<Vec<_>>().join("\n")
                )
              }
            } else {
              String::new()
            };
            bail!(
              "Could not find plugin with name '{}'. Please fix the name or try a url instead.{}",
              plugin_name_or_url,
              trailing_message,
            )
          }
        };
        for (config_plugin_reference, config_plugin) in get_config_file_plugins(plugin_resolver, config.plugins).await {
          if let Ok(config_plugin) = config_plugin {
            if let Some(update_url) = &config_plugin.info().update_url {
              if let Ok(Some(config_plugin_latest)) = read_update_url(&cached_downloader, update_url).await {
                // if two plugins have the same URL to be updated to then they're the same plugin
                if config_plugin_latest.url == plugin.url {
                  let file_text = environment.read_file(&config_path)?;
                  let new_reference = plugin.as_source_reference()?;
                  let file_text = update_plugin_in_config(
                    &file_text,
                    PluginUpdateInfo {
                      name: config_plugin.info().name.to_string(),
                      old_version: config_plugin.info().version.to_string(),
                      old_reference: config_plugin_reference,
                      new_version: plugin.version,
                      new_reference,
                    },
                  );
                  environment.write_file(&config_path, &file_text)?;
                  return Ok(());
                }
              }
            }
          }
        }
        plugin.full_url_no_wasm_checksum()
      }
    },
    None => {
      let mut possible_plugins = get_possible_plugins_to_add(environment, plugin_resolver, config.plugins).await?;
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

async fn get_possible_plugins_to_add<TEnvironment: Environment>(
  environment: &TEnvironment,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
  current_plugins: Vec<PluginSourceReference>,
) -> Result<Vec<InfoFilePluginInfo>> {
  let info_file = read_info_file(environment)
    .await
    .map_err(|err| anyhow!("Failed downloading info file. {:#}", err))?;
  let current_plugin_names = get_config_file_plugins(plugin_resolver, current_plugins)
    .await
    .into_iter()
    .filter_map(|(plugin_reference, plugin_result)| match plugin_result {
      Ok(plugin) => Some(plugin.info().name.to_string()),
      Err(err) => {
        log_warn!(environment, "Failed resolving plugin: {}\n\n{:#}", plugin_reference.path_source.display(), err);
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

pub async fn update_plugins_config_file<TEnvironment: Environment>(
  args: &CliArgs,
  environment: &TEnvironment,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
  yes_to_prompts: bool,
) -> Result<()> {
  if !args.plugins.is_empty() {
    bail!("Cannot specify plugins for this sub command. Sorry, too much work for me.");
  }

  let file_pattern_args = FilePatternArgs {
    file_patterns: Vec::new(),
    exclude_file_patterns: Vec::new(),
    allow_node_modules: false,
  };
  let scopes = resolve_plugins_scope_and_paths(args, &file_pattern_args, environment, plugin_resolver).await?;
  let mut plugin_responses = HashMap::new();
  for (i, scope) in scopes.into_iter().enumerate() {
    let is_main_config = i == 0;
    let Some(config) = &scope.scope.config else {
      continue;
    };
    let config_path = match &config.resolved_path.source {
      PathSource::Local(source) => &source.path,
      PathSource::Remote(source) => {
        log_warn!(environment, "Skipping remote configuration file: {}", source.url);
        continue;
      }
    };

    let mut file_text = environment.read_file(config_path)?;
    let plugins_to_update = get_plugins_to_update(environment, plugin_resolver, config.plugins.clone()).await?;

    for result in plugins_to_update {
      match result {
        Ok(info) => {
          let should_update = if info.is_wasm() || yes_to_prompts {
            true
          } else if let Some(previous_response) = plugin_responses.get(&info.new_reference) {
            *previous_response
          } else {
            // prompt for security reasons
            log_all!(
              environment,
              "The process plugin {} {} has a new url: {}",
              info.name,
              info.old_version,
              info.get_full_new_config_url(),
            );
            let response = environment.confirm("Do you want to update it?", false)?;
            plugin_responses.insert(info.new_reference.clone(), response);
            response
          };

          if should_update {
            log_stderr_info!(
              environment,
              "Updating {} {}{} to {}...",
              info.name,
              info.old_version,
              if is_main_config {
                String::new()
              } else {
                format!(" in {}", config_path.display())
              },
              info.new_version
            );
            file_text = update_plugin_in_config(&file_text, info);
          }
        }
        Err(err_info) => {
          log_warn!(environment, "Failed updating plugin {}: {:#}", err_info.name, err_info.error);
        }
      }
    }

    environment.write_file(config_path, &file_text)?;
  }

  // now resolve the plugins again in every scope and run their config updates

  run_plugin_config_updates(environment, args, &file_pattern_args, plugin_resolver)
    .await
    .with_context(|| "Failed running plugin config updates.".to_string())?;

  Ok(())
}

async fn run_plugin_config_updates<TEnvironment: Environment>(
  environment: &TEnvironment,
  args: &CliArgs,
  file_pattern_args: &FilePatternArgs,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
) -> Result<()> {
  let scopes = resolve_plugins_scope_and_paths(args, file_pattern_args, environment, plugin_resolver).await?;
  for scope in scopes.into_iter() {
    let Some(config) = &scope.scope.config else {
      continue;
    };
    let config_path = match &config.resolved_path.source {
      PathSource::Local(source) => &source.path,
      PathSource::Remote(_) => {
        continue;
      }
    };
    let mut file_text = environment.read_file(config_path)?;
    let config_map = match deserialize_config_raw(&file_text) {
      Ok(map) => map,
      Err(err) => {
        log_warn!(environment, "Failed deserializing config file '{}': {:#}", config_path.display(), err);
        continue;
      }
    };
    let mut all_diagnostics = Vec::new();
    for plugin in scope.scope.plugins.values() {
      log_debug!(environment, "Updating for {}", plugin.name());
      let config_key = &plugin.info().config_key;
      let Some(plugin_config) = config_map.get(config_key).and_then(|c| c.as_object()).cloned() else {
        continue;
      };
      let initialized_plugin = match plugin.initialize().await {
        Ok(plugin) => plugin,
        Err(err) => {
          log_warn!(environment, "Failed initializing {}. {:#}", plugin.name(), err);
          continue;
        }
      };

      let changes = match initialized_plugin.check_config_updates(plugin_config).await {
        Ok(changes) => changes,
        Err(err) => {
          log_warn!(environment, "Failed updating {}. {:#}", plugin.name(), err);
          continue;
        }
      };

      log_debug!(environment, "Had {} changes.", changes.len());
      if changes.is_empty() {
        continue;
      }

      let result = apply_config_changes(file_text, config_key, &changes);
      all_diagnostics.extend(result.diagnostics);
      file_text = result.new_text;
    }

    // apply the changes to the config
    if !all_diagnostics.is_empty() {
      log_warn!(environment, "Had diagnostics applying update config changes for {}:", config_path.display());
      for diagnostic in &all_diagnostics {
        log_warn!(environment, "* {}", diagnostic);
      }
    }
    environment.write_file(config_path, &file_text)?;
  }
  Ok(())
}

struct PluginUpdateError {
  name: String,
  error: Error,
}

async fn get_plugins_to_update<TEnvironment: Environment>(
  environment: &TEnvironment,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
  plugins: Vec<PluginSourceReference>,
) -> Result<Vec<Result<PluginUpdateInfo, PluginUpdateError>>> {
  async fn resolve_plugin_update_info<TEnvironment: Environment>(
    environment: &TEnvironment,
    plugin_reference: PluginSourceReference,
    plugin_result: Result<Rc<PluginWrapper>>,
  ) -> Option<Result<PluginUpdateInfo, PluginUpdateError>> {
    let plugin = match plugin_result {
      Ok(plugin) => plugin,
      Err(error) => {
        return Some(Err(PluginUpdateError {
          name: plugin_reference.path_source.display(),
          error,
        }))
      }
    };

    // request
    if let Some(plugin_update_url) = &plugin.info().update_url {
      match read_update_url(environment, plugin_update_url).await.and_then(|result| match result {
        Some(info) => match info.as_source_reference() {
          Ok(source_reference) => Ok((info, source_reference)),
          Err(err) => Err(err),
        },
        None => Err(anyhow!("Failed downloading {} - 404 Not Found", plugin_update_url)),
      }) {
        Ok((info, new_reference)) => Some(Ok(PluginUpdateInfo {
          name: plugin.info().name.to_string(),
          old_reference: plugin_reference,
          old_version: plugin.info().version.to_string(),
          new_version: info.version,
          new_reference,
        })),
        Err(err) => {
          // output and fallback to using the info file
          log_warn!(environment, "Failed reading plugin latest info. {:#}", err);
          None
        }
      }
    } else {
      log_warn!(
        environment,
        "Skipping {} as it did not specify an update url. Please update manually.",
        plugin.info().name
      );
      None
    }
  }

  let config_file_plugins = get_config_file_plugins(plugin_resolver, plugins).await;
  let mut final_infos = Vec::with_capacity(config_file_plugins.len());
  for (plugin_reference, plugin_result) in config_file_plugins {
    let maybe_info = resolve_plugin_update_info(environment, plugin_reference, plugin_result).await;
    if let Some(info) = maybe_info {
      if info.as_ref().ok().map(|info| info.old_version != info.new_version).unwrap_or(true) {
        final_infos.push(info);
      }
    }
  }
  Ok(final_infos)
}

pub async fn output_resolved_config<TEnvironment: Environment>(
  args: &CliArgs,
  environment: &TEnvironment,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
) -> Result<()> {
  let config = Rc::new(resolve_config_from_args(args, environment).await?);
  let plugins_scope = resolve_plugins_scope(config, environment, plugin_resolver).await?;
  plugins_scope.ensure_no_global_config_diagnostics()?;

  let mut plugin_jsons = Vec::new();
  for plugin in plugins_scope.plugins.values() {
    let config_key = &plugin.info().config_key;

    // output its diagnostics
    let plugin = match plugin.get_or_create_checking_config_diagnostics(environment).await? {
      GetPluginResult::HadDiagnostics(count) => bail!("Plugin had {} diagnostic(s)", count),
      GetPluginResult::Success(plugin) => plugin,
    };

    let text = plugin.resolved_config().await?;
    let pretty_text = pretty_print_json_text(&text)?;
    plugin_jsons.push(format!("\"{}\": {}", config_key, pretty_text));
  }

  environment.log_machine_readable(
    &if plugin_jsons.is_empty() {
      "{}".to_string()
    } else {
      let text = plugin_jsons.join(",\n").lines().map(|l| format!("  {}", l)).collect::<Vec<_>>().join("\n");
      format!("{{\n{}\n}}", text)
    }
    .into_bytes(),
  );

  Ok(())
}

async fn get_config_file_plugins<TEnvironment: Environment>(
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
  current_plugins: Vec<PluginSourceReference>,
) -> Vec<(PluginSourceReference, Result<Rc<PluginWrapper>>)> {
  let tasks = current_plugins
    .into_iter()
    .map(|plugin_reference| {
      let plugin_resolver = plugin_resolver.clone();
      dprint_core::async_runtime::spawn(async move {
        let resolve_result = plugin_resolver.resolve_plugin(plugin_reference.clone()).await;
        (plugin_reference, resolve_result)
      })
    })
    .collect::<Vec<_>>();

  let mut results = Vec::with_capacity(tasks.len());
  for result in future::join_all(tasks).await {
    results.push(result.unwrap());
  }
  results
}

#[cfg(test)]
mod test {
  use anyhow::Result;
  use once_cell::sync::Lazy;
  use pretty_assertions::assert_eq;
  use serde_json::json;

  use crate::assert_contains;
  use crate::configuration::*;
  use crate::environment::Environment;
  use crate::environment::TestEnvironment;
  use crate::environment::TestEnvironmentBuilder;
  use crate::environment::TestInfoFilePlugin;
  use crate::test_helpers;
  use crate::test_helpers::get_test_wasm_plugin_checksum;
  use crate::test_helpers::run_test_cli;
  use crate::test_helpers::TestProcessPluginFile;
  use crate::test_helpers::TestProcessPluginFileBuilder;

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
    let expected_text = environment.clone().run_in_runtime({
      let environment = environment.clone();
      async move {
        let expected_text = get_init_config_file_text(&environment).await.unwrap();
        environment.clear_logs();
        expected_text
      }
    });
    run_test_cli(vec!["init"], &environment).unwrap();
    assert_eq!(
      environment.take_stderr_messages(),
      vec!["Select plugins (use the spacebar to select/deselect and then press enter when finished):"]
    );
    assert_eq!(
      environment.take_stdout_messages(),
      vec![
        "\nCreated ./dprint.json",
        "\nIf you are working in a commercial environment please consider sponsoring dprint: https://dprint.dev/sponsor"
      ]
    );
    assert_eq!(environment.read_file("./dprint.json").unwrap(), expected_text);
  }

  #[test]
  fn should_use_dprint_config_init_as_alias() {
    let environment = TestEnvironment::new();
    let expected_text = environment.clone().run_in_runtime({
      let environment = environment.clone();
      async move {
        let expected_text = get_init_config_file_text(&environment).await.unwrap();
        environment.clear_logs();
        expected_text
      }
    });

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
    let expected_text = environment.clone().run_in_runtime({
      let environment = environment.clone();
      async move {
        let expected_text = get_init_config_file_text(&environment).await.unwrap();
        environment.clear_logs();
        expected_text
      }
    });
    run_test_cli(vec!["init", "--config", "./test.config.json"], &environment).unwrap();
    assert_eq!(
      environment.take_stderr_messages(),
      vec!["Select plugins (use the spacebar to select/deselect and then press enter when finished):"]
    );
    assert_eq!(
      environment.take_stdout_messages(),
      vec![
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
    let old_ps_checksum = OLD_PROCESS_PLUGIN_FILE.checksum();
    let old_ps_url = format!("https://plugins.dprint.dev/test-process.json@{}", old_ps_checksum);
    let new_ps_url = "https://plugins.dprint.dev/test-plugin-3.json".to_string();
    let new_ps_url_with_checksum = format!("{}@{}", new_ps_url, NEW_PROCESS_PLUGIN_FILE.checksum());
    let select_plugin_msg = "Select a plugin to add:".to_string();

    // no plugins specified
    test_add(TestAddOptions {
      add_arg: None,
      config_has_wasm: false,
      config_has_process: false,
      remote_has_checksums: false,
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
      remote_has_checksums: true,
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
      remote_has_checksums: false,
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
      remote_has_checksums: false,
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
      remote_has_checksums: false,
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
      remote_has_checksums: false,
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
      remote_has_checksums: false,
      expected_error: None,
      expected_logs: vec![],
      expected_urls: vec![new_wasm_url.clone()],
      selection_result: None,
    });

    // using arg and no existing plugin
    test_add(TestAddOptions {
      add_arg: Some("my-plugin"),
      config_has_wasm: false,
      config_has_process: false,
      remote_has_checksums: false,
      expected_error: Some(
        "Could not find plugin with name 'my-plugin'. Please fix the name or try a url instead.\n\nPlugins:\n * test-plugin\n * test-process-plugin",
      ),
      expected_logs: vec![],
      expected_urls: vec![],
      selection_result: None,
    });

    // using and already exists
    test_add(TestAddOptions {
      add_arg: Some("test-plugin"),
      config_has_wasm: true,
      config_has_process: false,
      remote_has_checksums: false,
      expected_error: None,
      expected_logs: vec![],
      expected_urls: vec![
        // upgrades to the latest
        new_wasm_url,
      ],
      selection_result: None,
    });

    // using url
    test_add(TestAddOptions {
      add_arg: Some("https://plugins.dprint.dev/my-plugin.wasm"),
      config_has_wasm: false,
      config_has_process: false,
      remote_has_checksums: false,
      expected_error: None,
      expected_logs: vec![],
      expected_urls: vec!["https://plugins.dprint.dev/my-plugin.wasm".to_string()],
      selection_result: None,
    });
  }

  #[derive(Debug)]
  struct TestAddOptions {
    add_arg: Option<&'static str>,
    config_has_wasm: bool,
    config_has_process: bool,
    remote_has_checksums: bool,
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
      remote_has_wasm_checksum: options.remote_has_checksums,
      remote_has_process_checksum: options.remote_has_checksums,
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
  fn config_update_should_always_upgrade_to_latest_plugins() {
    let new_wasm_url = "https://plugins.dprint.dev/test-plugin-2.wasm".to_string();
    // test all the process plugin combinations
    let new_ps_url = "https://plugins.dprint.dev/test-plugin-3.json".to_string();
    let new_ps_url_with_checksum = format!("{}@{}", new_ps_url, NEW_PROCESS_PLUGIN_FILE.checksum());
    test_update(TestUpdateOptions {
      config_has_wasm: false,
      config_has_wasm_checksum: false,
      config_has_process: true,
      remote_has_wasm_checksum: true,
      remote_has_process_checksum: true,
      confirm_results: Vec::new(),
      expected_logs: vec![
        "Updating test-process-plugin 0.1.0 to 0.3.0...".to_string(),
        "Extracting zip for test-process-plugin".to_string(),
      ],
      expected_urls: vec![new_ps_url_with_checksum.clone()],
      always_update: true,
      on_error: None,
      exit_code: 0,
    });
    test_update(TestUpdateOptions {
      config_has_wasm: false,
      config_has_wasm_checksum: false,
      config_has_process: true,
      remote_has_wasm_checksum: false,
      remote_has_process_checksum: false,
      confirm_results: Vec::new(),
      expected_logs: vec!["Updating test-process-plugin 0.1.0 to 0.3.0...".to_string()],
      expected_urls: vec![new_ps_url.clone()],
      always_update: true,
      on_error: Some(Box::new(|text| {
        assert_contains!(text, "Error resolving plugin https://plugins.dprint.dev/test-plugin-3.json: The plugin must have a checksum specified for security reasons since it is not a Wasm plugin.");
      })),
      exit_code: 12,
    });
    test_update(TestUpdateOptions {
      config_has_wasm: false,
      config_has_wasm_checksum: false,
      config_has_process: true,
      remote_has_wasm_checksum: false,
      remote_has_process_checksum: true,
      confirm_results: Vec::new(),
      expected_logs: vec![
        "Updating test-process-plugin 0.1.0 to 0.3.0...".to_string(),
        "Extracting zip for test-process-plugin".to_string(),
      ],
      expected_urls: vec![new_ps_url_with_checksum.clone()],
      always_update: true,
      on_error: None,
      exit_code: 0,
    });

    test_update(TestUpdateOptions {
      config_has_wasm: true,
      config_has_wasm_checksum: false,
      config_has_process: true,
      remote_has_wasm_checksum: false,
      remote_has_process_checksum: true,
      confirm_results: Vec::new(),
      expected_logs: vec![
        "Updating test-plugin 0.1.0 to 0.2.0...".to_string(),
        "Updating test-process-plugin 0.1.0 to 0.3.0...".to_string(),
        "Compiling https://plugins.dprint.dev/test-plugin-2.wasm".to_string(),
        "Extracting zip for test-process-plugin".to_string(),
      ],
      expected_urls: vec![new_wasm_url.clone(), new_ps_url_with_checksum.clone()],
      always_update: true,
      on_error: None,
      exit_code: 0,
    });
  }

  #[test]
  fn config_update_should_upgrade_to_latest_plugins() {
    let new_wasm_url = "https://plugins.dprint.dev/test-plugin-2.wasm".to_string();
    let new_wasm_url_with_checksum = format!("{}@{}", new_wasm_url, get_test_wasm_plugin_checksum());
    let updating_message = "Updating test-plugin 0.1.0 to 0.2.0...".to_string();
    let compiling_message = "Compiling https://plugins.dprint.dev/test-plugin-2.wasm".to_string();

    // test all the wasm combinations
    test_update(TestUpdateOptions {
      config_has_wasm: true,
      config_has_wasm_checksum: true,
      config_has_process: false,
      remote_has_wasm_checksum: true,
      remote_has_process_checksum: true,
      confirm_results: Vec::new(),
      expected_logs: vec![updating_message.clone(), compiling_message.clone()],
      expected_urls: vec![new_wasm_url_with_checksum.clone()],
      always_update: false,
      on_error: None,
      exit_code: 0,
    });
    test_update(TestUpdateOptions {
      config_has_wasm: true,
      config_has_wasm_checksum: true,
      config_has_process: false,
      remote_has_wasm_checksum: false,
      remote_has_process_checksum: true,
      confirm_results: Vec::new(),
      expected_logs: vec![updating_message.clone(), compiling_message.clone()],
      expected_urls: vec![new_wasm_url.clone()],
      always_update: false,
      on_error: None,
      exit_code: 0,
    });
    test_update(TestUpdateOptions {
      config_has_wasm: true,
      config_has_wasm_checksum: false,
      config_has_process: false,
      remote_has_wasm_checksum: true,
      remote_has_process_checksum: true,
      confirm_results: Vec::new(),
      expected_logs: vec![updating_message.clone(), compiling_message.clone()],
      expected_urls: vec![new_wasm_url.clone()],
      always_update: false,
      on_error: None,
      exit_code: 0,
    });
    test_update(TestUpdateOptions {
      config_has_wasm: true,
      config_has_wasm_checksum: false,
      config_has_process: false,
      remote_has_wasm_checksum: false,
      remote_has_process_checksum: true,
      confirm_results: Vec::new(),
      expected_logs: vec![updating_message.clone(), compiling_message.clone()],
      expected_urls: vec![new_wasm_url.clone()],
      always_update: false,
      on_error: None,
      exit_code: 0,
    });

    // test all the process plugin combinations
    let old_ps_checksum = TestProcessPluginFile::default().checksum();
    let old_ps_url = format!("https://plugins.dprint.dev/test-process.json@{}", old_ps_checksum);
    let new_ps_url = "https://plugins.dprint.dev/test-plugin-3.json".to_string();
    let new_ps_url_with_checksum = format!("{}@{}", new_ps_url, NEW_PROCESS_PLUGIN_FILE.checksum());
    test_update(TestUpdateOptions {
      config_has_wasm: false,
      config_has_wasm_checksum: false,
      config_has_process: true,
      remote_has_wasm_checksum: true,
      remote_has_process_checksum: true,
      confirm_results: vec![Ok(Some(true))],
      expected_logs: vec![
        format!("The process plugin test-process-plugin 0.1.0 has a new url: {}", new_ps_url_with_checksum),
        "Do you want to update it? Y".to_string(),
        "Updating test-process-plugin 0.1.0 to 0.3.0...".to_string(),
        "Extracting zip for test-process-plugin".to_string(),
      ],
      expected_urls: vec![new_ps_url_with_checksum.clone()],
      always_update: false,
      on_error: None,
      exit_code: 0,
    });
    test_update(TestUpdateOptions {
      config_has_wasm: false,
      config_has_wasm_checksum: false,
      config_has_process: true,
      remote_has_wasm_checksum: false,
      remote_has_process_checksum: false,
      confirm_results: vec![Ok(Some(true))],
      expected_logs: vec![
        format!("The process plugin test-process-plugin 0.1.0 has a new url: {}", new_ps_url),
        "Do you want to update it? Y".to_string(),
        "Updating test-process-plugin 0.1.0 to 0.3.0...".to_string(),
      ],
      expected_urls: vec![new_ps_url.clone()],
      always_update: false,
      on_error: Some(Box::new(|text| {
        assert_contains!(text, "Error resolving plugin https://plugins.dprint.dev/test-plugin-3.json: The plugin must have a checksum specified for security reasons since it is not a Wasm plugin.");
      })),
      exit_code: 12,
    });
    test_update(TestUpdateOptions {
      config_has_wasm: false,
      config_has_wasm_checksum: false,
      config_has_process: true,
      remote_has_wasm_checksum: false,
      remote_has_process_checksum: false,
      confirm_results: vec![Ok(None)],
      expected_logs: vec![
        format!("The process plugin test-process-plugin 0.1.0 has a new url: {}", new_ps_url),
        "Do you want to update it? N".to_string(),
      ],
      expected_urls: vec![old_ps_url.clone()],
      always_update: false,
      on_error: None,
      exit_code: 0,
    });

    // testing both in config, but only updating one
    test_update(TestUpdateOptions {
      config_has_wasm: true,
      config_has_wasm_checksum: false,
      config_has_process: true,
      remote_has_wasm_checksum: false,
      remote_has_process_checksum: true,
      confirm_results: vec![Ok(Some(false))],
      expected_logs: vec![
        "Updating test-plugin 0.1.0 to 0.2.0...".to_string(),
        format!("The process plugin test-process-plugin 0.1.0 has a new url: {}", new_ps_url_with_checksum),
        "Do you want to update it? N".to_string(),
        "Compiling https://plugins.dprint.dev/test-plugin-2.wasm".to_string(),
      ],
      expected_urls: vec![new_wasm_url.clone(), old_ps_url.clone()],
      always_update: false,
      on_error: None,
      exit_code: 0,
    });
  }

  #[test]
  fn config_update_plugin_config() {
    let mut builder = get_setup_builder(SetupEnvOptions {
      config_has_wasm: true,
      config_has_wasm_checksum: false,
      config_has_process: true,
      remote_has_wasm_checksum: false,
      remote_has_process_checksum: true,
    });
    builder.with_default_config(|config| {
      config.add_config_section(
        "testProcessPlugin",
        r#"{
  "should_add": {
  },
  "should_set": "other",
  "should_remove": {},
}"#,
      );
    });
    builder.with_local_config("/sub_folder/dprint.json", |config| {
      config.add_remote_process_plugin().add_config_section(
        "testProcessPlugin",
        r#"{
  "should_set": "asdf"
}"#,
      );
    });
    let environment = builder.initialize().build();
    run_test_cli(vec!["config", "update", "--yes"], &environment).unwrap();
    assert_eq!(
      environment.take_stderr_messages(),
      vec![
        "Updating test-plugin 0.1.0 to 0.2.0...".to_string(),
        "Updating test-process-plugin 0.1.0 to 0.3.0...".to_string(),
        "Updating test-process-plugin 0.1.0 in /sub_folder/dprint.json to 0.3.0...".to_string(),
        "Compiling https://plugins.dprint.dev/test-plugin-2.wasm".to_string(),
        "Extracting zip for test-process-plugin".to_string()
      ]
    );
    assert_eq!(
      environment.read_file("./dprint.json").unwrap(),
      format!(
        r#"{{
  "testProcessPlugin": {{
    "should_add": "new_value",
    "should_set": "new_value",
    "new_prop1": [
      "new_value"
    ],
    "new_prop2": {{
      "new_prop": "new_value"
    }},
  }},
  "plugins": [
    "https://plugins.dprint.dev/test-plugin-2.wasm",
    "https://plugins.dprint.dev/test-plugin-3.json@{}"
  ]
}}"#,
        NEW_PROCESS_PLUGIN_FILE.checksum()
      )
    );
    assert_eq!(
      environment.read_file("./sub_folder/dprint.json").unwrap(),
      format!(
        r#"{{
  "testProcessPlugin": {{
    "should_set": "new_value"
  }},
  "plugins": [
    "https://plugins.dprint.dev/test-plugin-3.json@{}"
  ]
}}"#,
        NEW_PROCESS_PLUGIN_FILE.checksum()
      )
    );
  }

  struct TestUpdateOptions {
    config_has_wasm: bool,
    config_has_wasm_checksum: bool,
    config_has_process: bool,
    remote_has_wasm_checksum: bool,
    remote_has_process_checksum: bool,
    confirm_results: Vec<Result<Option<bool>>>,
    expected_logs: Vec<String>,
    expected_urls: Vec<String>,
    always_update: bool,
    on_error: Option<Box<dyn FnOnce(&str)>>,
    exit_code: i32,
  }

  #[track_caller]
  fn test_update(options: TestUpdateOptions) {
    let expected_logs = options.expected_logs.clone();
    let expected_urls = options.expected_urls.clone();
    let environment = get_setup_env(SetupEnvOptions {
      config_has_wasm: options.config_has_wasm,
      config_has_wasm_checksum: options.config_has_wasm_checksum,
      config_has_process: options.config_has_process,
      remote_has_wasm_checksum: options.remote_has_wasm_checksum,
      remote_has_process_checksum: options.remote_has_process_checksum,
    });
    environment.set_confirm_results(options.confirm_results);

    let result = run_test_cli(
      if options.always_update {
        vec!["config", "update", "--yes"]
      } else {
        vec!["config", "update"]
      },
      &environment,
    );
    if let Err(err) = result {
      let on_error = match options.on_error {
        Some(on_error) => on_error,
        None => panic!("{:#}", err),
      };
      (on_error)(&err.to_string());
      err.assert_exit_code(options.exit_code);
    } else {
      assert_eq!(options.on_error.is_some(), false);
      assert_eq!(options.exit_code, 0);
    }
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

  static OLD_PROCESS_PLUGIN_FILE: Lazy<TestProcessPluginFile> = Lazy::new(|| TestProcessPluginFileBuilder::default().version("0.1.0").build());
  static NEW_PROCESS_PLUGIN_FILE: Lazy<TestProcessPluginFile> = Lazy::new(|| TestProcessPluginFileBuilder::default().version("0.3.0").build());

  #[derive(Debug)]
  struct SetupEnvOptions {
    config_has_wasm: bool,
    config_has_wasm_checksum: bool,
    config_has_process: bool,
    remote_has_wasm_checksum: bool,
    remote_has_process_checksum: bool,
  }

  fn get_setup_env(opts: SetupEnvOptions) -> TestEnvironment {
    get_setup_builder(opts).initialize().build()
  }

  fn get_setup_builder(opts: SetupEnvOptions) -> TestEnvironmentBuilder {
    let actual_wasm_plugin_checksum = test_helpers::get_test_wasm_plugin_checksum();
    let mut builder = TestEnvironmentBuilder::new();

    if opts.config_has_wasm {
      builder.add_remote_wasm_plugin();
      builder.add_remote_wasm_plugin_at_url("https://plugins.dprint.dev/test-plugin-2.wasm");
    }
    if opts.config_has_process {
      builder.add_remote_process_plugin();
      builder.add_remote_process_plugin_at_url("https://plugins.dprint.dev/test-plugin-3.json", &*NEW_PROCESS_PLUGIN_FILE);
    }

    builder
      .with_info_file(|info| {
        info.add_plugin(TestInfoFilePlugin {
          name: "test-plugin".to_string(),
          version: "0.2.0".to_string(),
          url: "https://plugins.dprint.dev/test-plugin-2.wasm".to_string(),
          config_key: Some("test-plugin".to_string()),
          checksum: if opts.remote_has_wasm_checksum {
            Some(get_test_wasm_plugin_checksum())
          } else {
            None
          },
          ..Default::default()
        });

        info.add_plugin(TestInfoFilePlugin {
          name: "test-process-plugin".to_string(),
          version: "0.3.0".to_string(),
          url: "https://plugins.dprint.dev/test-plugin-3.json".to_string(),
          config_key: Some("test-process-plugin".to_string()),
          checksum: if opts.remote_has_process_checksum {
            Some(NEW_PROCESS_PLUGIN_FILE.checksum())
          } else {
            None
          },
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
      })
      .add_remote_file(
        "https://plugins.dprint.dev/dprint/test-plugin/latest.json",
        &json!({
          "schemaVersion": 1,
          "url": "https://plugins.dprint.dev/test-plugin-2.wasm",
          "version": "0.2.0",
          "checksum": if opts.remote_has_wasm_checksum { Some(get_test_wasm_plugin_checksum()) } else { None },
        })
        .to_string(),
      )
      .add_remote_file(
        "https://plugins.dprint.dev/dprint/test-process-plugin/latest.json",
        &json!({
          "schemaVersion": 1,
          "url": "https://plugins.dprint.dev/test-plugin-3.json",
          "version": "0.3.0",
          "checksum": if opts.remote_has_process_checksum { Some(NEW_PROCESS_PLUGIN_FILE.checksum()) } else { None },
        })
        .to_string(),
      );
    builder
  }

  #[test]
  fn config_update_should_not_upgrade_when_at_latest_plugins() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_info_file(|_| {})
      .with_default_config(|config| {
        config.add_remote_wasm_plugin();
      })
      .add_remote_file(
        "https://plugins.dprint.dev/dprint/test-plugin/latest.json",
        &json!({
          "schemaVersion": 1,
          "url": "https://plugins.dprint.dev/test-plugin-2.wasm",
          "version": "0.1.0"
        })
        .to_string(),
      )
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
      .with_info_file(|_| {})
      .with_default_config(|config| {
        config.add_remote_wasm_plugin();
      })
      .add_remote_file(
        "https://plugins.dprint.dev/dprint/test-plugin/latest.json",
        &json!({
          "schemaVersion": 1,
          "url": "https://plugins.dprint.dev/test-plugin.json",
          "version": "0.2.0",
          "checksum": "checksum",
        })
        .to_string(),
      )
      .initialize()
      .build();
    environment.set_confirm_results(vec![Ok(None)]);
    run_test_cli(vec!["config", "update"], &environment).unwrap();
    assert_eq!(
      environment.take_stderr_messages(),
      vec![
        "The process plugin test-plugin 0.1.0 has a new url: https://plugins.dprint.dev/test-plugin.json@checksum",
        "Do you want to update it? N"
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
