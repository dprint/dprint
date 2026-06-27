use std::collections::HashSet;
use std::collections::VecDeque;

use anyhow::Result;
use dprint_core::plugins::wasm::{self};

use crate::environment::DirEntry;
use crate::environment::Environment;
use crate::plugins::InfoFilePluginInfo;
use crate::plugins::read_info_file;

/// Maximum number of files to look at when scanning the current directory to
/// decide which plugins to pre-select. Keeps `dprint init` fast in large repos.
const MAX_SCAN_FILES: usize = 1000;
/// Upper bound on directories visited during the scan so a deep tree with few
/// files can't make the scan walk forever.
const MAX_SCAN_DIRS: usize = 1000;

#[derive(Default)]
pub struct GetInitConfigFileTextOptions {
  /// Skip the interactive plugin prompt and accept the plugins selected based
  /// on the files in the current directory.
  pub non_interactive: bool,
}

pub async fn get_init_config_file_text(environment: &impl Environment, options: GetInitConfigFileTextOptions) -> Result<String> {
  let info = match read_info_file(environment).await {
    Ok(info) => {
      // ok to only check wasm here because the configuration file is only ever initialized with wasm plugins
      if wasm::PLUGIN_SYSTEM_SCHEMA_VERSION < info.plugin_system_schema_version {
        log_error!(
          environment,
          concat!(
            "You are using an old version of dprint so the created config file may not be as helpful of a starting point. ",
            "Consider upgrading to support new plugins. ",
            "Plugin system schema version is {}, latest is {}."
          ),
          wasm::PLUGIN_SYSTEM_SCHEMA_VERSION,
          info.plugin_system_schema_version,
        );
        None
      } else {
        Some(info)
      }
    }
    Err(err) => {
      log_error!(
        environment,
        concat!(
          "There was a problem getting the latest plugin info. ",
          "The created config file may not be as helpful of a starting point. ",
          "Error: {}"
        ),
        err,
      );
      None
    }
  };

  let selected_plugins = if let Some(info) = info {
    let latest_plugins = info.latest_plugins;
    // pre-select the plugins that match files found in the current directory
    let project_files = scan_project_files(environment);
    let defaults = latest_plugins
      .iter()
      .map(|plugin| is_default_selected(plugin, &project_files))
      .collect::<Vec<_>>();

    let selected_indexes = if options.non_interactive {
      defaults.iter().enumerate().filter_map(|(i, on)| on.then_some(i)).collect::<Vec<_>>()
    } else {
      let prompt_message = "Select plugins (space to toggle, type to filter, enter to finish):";
      let items = latest_plugins
        .iter()
        .zip(defaults.iter())
        .map(|(plugin, default)| (*default, plugin_display_text(plugin)))
        .collect::<Vec<_>>();
      environment.get_multi_selection(prompt_message, 0, &items)?
    };

    let mut selected_plugins = Vec::new();
    for index in selected_indexes {
      selected_plugins.push(latest_plugins[index].clone());
    }
    Some(selected_plugins)
  } else {
    None
  };

  let mut json_text = String::from("{\n");

  if let Some(selected_plugins) = &selected_plugins {
    for plugin in selected_plugins.iter() {
      // Put the brace on the next line so the user doesn't have to as soon as they
      // go to add options.
      if let Some(config_key) = &plugin.config_key
        && !config_key.is_empty()
      {
        json_text.push_str(&format!("  \"{}\": {{\n", config_key));
        json_text.push_str("  },\n");
      }
    }

    json_text.push_str("  \"excludes\": [");
    let excludes = get_unique_items(
      selected_plugins
        .iter()
        .flat_map(|p| p.config_excludes.iter())
        .map(|x| format!("    \"{}\"", x))
        .collect::<Vec<_>>(),
    );
    if !excludes.is_empty() {
      json_text.push('\n');
      json_text.push_str(&excludes.join(",\n"));
      json_text.push_str("\n  ");
    }
    json_text.push_str("],\n");
    json_text.push_str("  \"plugins\": [\n");
    if selected_plugins.is_empty() {
      json_text.push_str("    // specify plugin urls here\n");
    } else {
      for (i, plugin) in selected_plugins.iter().enumerate() {
        if i > 0 {
          json_text.push_str(",\n");
        }
        let url = if plugin.is_process_plugin() && plugin.checksum.is_some() {
          format!("{}@{}", plugin.url, plugin.checksum.as_ref().unwrap())
        } else {
          plugin.url.to_string()
        };
        json_text.push_str(&format!("    \"{}\"", url));
      }
      json_text.push('\n');
    }
    json_text.push_str("  ]\n}\n");
  } else {
    json_text.push_str("  \"excludes\": [\n    \"**/*-lock.json\"\n  ],\n");
    json_text.push_str("  \"plugins\": [\n");
    json_text.push_str("    // specify plugin urls here\n");
    json_text.push_str("  ]\n}\n");
  }

  Ok(json_text)
}

/// The files found while scanning the current directory, used to decide which
/// plugins to pre-select.
struct ProjectFiles {
  /// Lowercased file extensions without the leading dot (ex. `ts`, `json`).
  extensions: HashSet<String>,
  file_names: HashSet<String>,
}

/// Whether the plugin should be pre-selected based on the files in the current
/// directory. Extensions are matched case-insensitively, while file names are
/// matched exactly because their casing is significant (ex. `Cargo.toml`).
fn is_default_selected(plugin: &InfoFilePluginInfo, project_files: &ProjectFiles) -> bool {
  plugin.file_extensions.iter().any(|ext| project_files.extensions.contains(&ext.to_lowercase()))
    || plugin.file_names.iter().any(|name| project_files.file_names.contains(name))
}

/// The text shown for a plugin in the selection list. The supported file
/// extensions are appended so unfamiliar plugins are easier to tell apart.
fn plugin_display_text(plugin: &InfoFilePluginInfo) -> String {
  if plugin.file_extensions.is_empty() {
    plugin.name.clone()
  } else {
    let extensions = plugin.file_extensions.iter().map(|ext| format!(".{}", ext)).collect::<Vec<_>>().join(", ");
    format!("{} ({})", plugin.name, extensions)
  }
}

/// Scans the current directory for files in order to decide which plugins to
/// pre-select. The scan is bounded by [`MAX_SCAN_FILES`] and [`MAX_SCAN_DIRS`]
/// to stay fast in large repositories and best-effort ignores errors.
fn scan_project_files(environment: &impl Environment) -> ProjectFiles {
  let mut extensions = HashSet::new();
  let mut file_names = HashSet::new();
  let mut files_remaining = MAX_SCAN_FILES;
  let mut dirs_remaining = MAX_SCAN_DIRS;
  let mut pending_dirs = VecDeque::from([environment.cwd().into_path_buf()]);

  'outer: while let Some(dir) = pending_dirs.pop_front() {
    if dirs_remaining == 0 {
      break;
    }
    dirs_remaining -= 1;
    let Ok(entries) = environment.dir_info(&dir) else {
      continue; // best-effort: skip directories we can't read
    };
    for entry in entries {
      match entry {
        DirEntry::Directory(path) => {
          if path.file_name().and_then(|name| name.to_str()).is_some_and(|name| !is_ignored_dir(name)) {
            pending_dirs.push_back(path);
          }
        }
        DirEntry::File { name, path } => {
          if files_remaining == 0 {
            break 'outer;
          }
          files_remaining -= 1;
          file_names.insert(name.to_string_lossy().into_owned());
          if let Some(ext) = path.extension().and_then(|ext| ext.to_str()) {
            extensions.insert(ext.to_lowercase());
          }
        }
      }
    }
  }

  ProjectFiles { extensions, file_names }
}

/// Directories that are skipped while scanning. Hidden directories (those
/// starting with a dot) are always skipped.
fn is_ignored_dir(name: &str) -> bool {
  name.starts_with('.') || matches!(name, "node_modules" | "target" | "vendor" | "dist" | "build" | "out" | "bin" | "obj")
}

/// Gets the unique items in the vector in the same order
fn get_unique_items<T>(vec: Vec<T>) -> Vec<T>
where
  T: PartialEq,
{
  let mut new_vec = Vec::new();

  for item in vec {
    if !new_vec.contains(&item) {
      new_vec.push(item);
    }
  }

  new_vec
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::environment::TestEnvironment;
  use crate::environment::TestEnvironmentBuilder;
  use crate::environment::TestInfoFilePlugin;
  use pretty_assertions::assert_eq;

  #[test]
  fn should_get_default_initialization_text() {
    // the typescript and json plugins are pre-selected because of these files
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        for plugin in get_multi_plugins_config() {
          info.add_plugin(plugin);
        }
      })
      .write_file("/file.ts", "")
      .write_file("/file.json", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      assert_eq!(
        text,
        r#"{
  "typescript": {
  },
  "json": {
  },
  "excludes": [
    "**/something",
    "**/*-asdf.json"
  ],
  "plugins": [
    "https://plugins.dprint.dev/typescript-0.17.2.wasm",
    "https://plugins.dprint.dev/json-0.2.3.wasm"
  ]
}
"#
      );

      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_select_no_plugins_when_no_files_match() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        for plugin in get_multi_plugins_config() {
          info.add_plugin(plugin);
        }
      })
      .write_file("/readme.md", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      assert_eq!(
        text,
        r#"{
  "excludes": [],
  "plugins": [
    // specify plugin urls here
  ]
}
"#
      );

      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_pre_select_plugin_by_file_name() {
    // the "final" plugin is matched via the Cargo.toml file name
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        for plugin in get_multi_plugins_config() {
          info.add_plugin(plugin);
        }
      })
      .write_file("/Cargo.toml", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      assert_eq!(
        text,
        r#"{
  "excludes": [
    "**/something",
    "**other"
  ],
  "plugins": [
    "https://plugins.dprint.dev/final-0.1.2.wasm"
  ]
}
"#
      );

      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_pre_select_process_plugin_by_extension() {
    // the process plugin is matched via its ".ps" file extension
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        for plugin in get_multi_plugins_config() {
          info.add_plugin(plugin);
        }
      })
      .write_file("/file.ps", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      assert_eq!(
        text,
        r#"{
  "excludes": [],
  "plugins": [
    "https://plugins.dprint.dev/process-0.1.0.json@test-checksum"
  ]
}
"#
      );

      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_scan_files_in_nested_directories() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        for plugin in get_multi_plugins_config() {
          info.add_plugin(plugin);
        }
      })
      .write_file("/src/nested/app.ts", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      // the typescript plugin is selected from the nested file
      assert!(text.contains("https://plugins.dprint.dev/typescript-0.17.2.wasm"), "{text}");
      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_not_scan_ignored_directories() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        for plugin in get_multi_plugins_config() {
          info.add_plugin(plugin);
        }
      })
      // matching files only exist within ignored directories
      .write_file("/node_modules/dep/app.ts", "")
      .write_file("/.git/hooks/config.json", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      assert_eq!(
        text,
        r#"{
  "excludes": [],
  "plugins": [
    // specify plugin urls here
  ]
}
"#
      );
      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_match_extensions_case_insensitively() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        for plugin in get_multi_plugins_config() {
          info.add_plugin(plugin);
        }
      })
      .write_file("/MAIN.TS", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      assert!(text.contains("https://plugins.dprint.dev/typescript-0.17.2.wasm"), "{text}");
      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_skip_prompt_when_non_interactive() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        for plugin in get_multi_plugins_config() {
          info.add_plugin(plugin);
        }
      })
      .write_file("/file.ts", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, GetInitConfigFileTextOptions { non_interactive: true })
        .await
        .unwrap();
      assert_eq!(
        text,
        r#"{
  "typescript": {
  },
  "excludes": [
    "**/something"
  ],
  "plugins": [
    "https://plugins.dprint.dev/typescript-0.17.2.wasm"
  ]
}
"#
      );

      // no prompt should be shown when non-interactive
      assert_eq!(environment.take_stderr_messages(), Vec::<String>::new());
    });
  }

  #[test]
  fn should_get_initialization_text_when_can_access_url() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        for plugin in get_multi_plugins_config() {
          info.add_plugin(plugin);
        }
      })
      .build();
    environment.set_multi_selection_result(vec![0, 1, 2]);
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      assert_eq!(
        text,
        r#"{
  "typescript": {
  },
  "json": {
  },
  "excludes": [
    "**/something",
    "**/*-asdf.json",
    "**other"
  ],
  "plugins": [
    "https://plugins.dprint.dev/typescript-0.17.2.wasm",
    "https://plugins.dprint.dev/json-0.2.3.wasm",
    "https://plugins.dprint.dev/final-0.1.2.wasm"
  ]
}
"#
      );

      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_get_initialization_text_when_selecting_one_plugin() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        for plugin in get_multi_plugins_config() {
          info.add_plugin(plugin);
        }
      })
      .build();
    environment.set_multi_selection_result(vec![1]);
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      assert_eq!(
        text,
        r#"{
  "json": {
  },
  "excludes": [
    "**/*-asdf.json"
  ],
  "plugins": [
    "https://plugins.dprint.dev/json-0.2.3.wasm"
  ]
}
"#
      );

      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_get_initialization_text_when_selecting_no_plugins() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        for plugin in get_multi_plugins_config() {
          info.add_plugin(plugin);
        }
      })
      .build();
    environment.set_multi_selection_result(vec![]);
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      assert_eq!(
        text,
        r#"{
  "excludes": [],
  "plugins": [
    // specify plugin urls here
  ]
}
"#
      );

      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_get_initialization_text_when_selecting_process_plugin() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        for plugin in get_multi_plugins_config() {
          info.add_plugin(plugin);
        }
      })
      .build();
    environment.set_multi_selection_result(vec![3]);
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      assert_eq!(
        text,
        r#"{
  "excludes": [],
  "plugins": [
    "https://plugins.dprint.dev/process-0.1.0.json@test-checksum"
  ]
}
"#
      );

      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_get_initialization_text_when_cannot_access_url() {
    let environment = TestEnvironment::new();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      assert_eq!(
        text,
        r#"{
  "excludes": [
    "**/*-lock.json"
  ],
  "plugins": [
    // specify plugin urls here
  ]
}
"#
      );
      let mut expected_messages = get_standard_logged_messages_no_plugin_selection();
      expected_messages.push(concat!(
        "There was a problem getting the latest plugin info. ",
        "The created config file may not be as helpful of a starting point. ",
        "Error: Error downloading https://plugins.dprint.dev/info.json - 404 Not Found"
      ));
      assert_eq!(environment.take_stderr_messages(), expected_messages);
    });
  }

  #[test]
  fn should_get_initialization_text_when_selecting_other_option() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info.add_plugin(TestInfoFilePlugin {
          name: "dprint-plugin-typescript".to_string(),
          version: "0.17.2".to_string(),
          url: "https://plugins.dprint.dev/typescript-0.17.2.wasm".to_string(),
          config_key: Some("typescript".to_string()),
          file_extensions: vec!["ts".to_string()],
          config_excludes: vec!["test".to_string()],
          ..Default::default()
        });
      })
      .build();
    environment.set_selection_result(1);
    environment.set_multi_selection_result(vec![0]);
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      assert_eq!(
        text,
        r#"{
  "typescript": {
  },
  "excludes": [
    "test"
  ],
  "plugins": [
    "https://plugins.dprint.dev/typescript-0.17.2.wasm"
  ]
}
"#
      );

      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_get_initialization_text_when_old_plugin_system() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info.set_plugin_schema_version(999).add_plugin(TestInfoFilePlugin {
          name: "dprint-plugin-typescript".to_string(),
          version: "0.17.2".to_string(),
          url: "https://plugins.dprint.dev/typescript-0.17.2.wasm".to_string(),
          config_key: Some("typescript".to_string()),
          file_extensions: vec!["ts".to_string()],
          config_excludes: vec!["asdf".to_string()],
          ..Default::default()
        });
      })
      .build();
    environment.set_multi_selection_result(vec![0]);
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      assert_eq!(
        text,
        r#"{
  "excludes": [
    "**/*-lock.json"
  ],
  "plugins": [
    // specify plugin urls here
  ]
}
"#
      );
      let mut expected_messages = get_standard_logged_messages_no_plugin_selection();
      expected_messages.push(concat!(
        "You are using an old version of dprint so the created config file may not be as helpful of a starting point. ",
        "Consider upgrading to support new plugins. ",
        "Plugin system schema version is 4, latest is 999."
      ));
      assert_eq!(environment.take_stderr_messages(), expected_messages);
    });
  }

  fn get_standard_logged_messages_no_plugin_selection() -> Vec<&'static str> {
    vec![]
  }

  fn get_standard_logged_messages() -> Vec<&'static str> {
    vec!["Select plugins (space to toggle, type to filter, enter to finish):"]
  }

  fn get_multi_plugins_config() -> Vec<TestInfoFilePlugin> {
    vec![
      TestInfoFilePlugin {
        name: "dprint-plugin-typescript".to_string(),
        version: "0.17.2".to_string(),
        url: "https://plugins.dprint.dev/typescript-0.17.2.wasm".to_string(),
        config_key: Some("typescript".to_string()),
        file_extensions: vec!["ts".to_string(), "tsx".to_string()],
        config_excludes: vec!["**/something".to_string()],
        ..Default::default()
      },
      TestInfoFilePlugin {
        name: "dprint-plugin-jsonc".to_string(),
        version: "0.2.3".to_string(),
        url: "https://plugins.dprint.dev/json-0.2.3.wasm".to_string(),
        config_key: Some("json".to_string()),
        file_extensions: vec!["json".to_string()],
        config_excludes: vec!["**/*-asdf.json".to_string()],
        ..Default::default()
      },
      TestInfoFilePlugin {
        name: "dprint-plugin-final".to_string(),
        version: "0.1.2".to_string(),
        url: "https://plugins.dprint.dev/final-0.1.2.wasm".to_string(),
        file_names: Some(vec!["Cargo.toml".to_string()]),
        file_extensions: vec!["tsx".to_string(), "rs".to_string()],
        config_excludes: vec!["**/something".to_string(), "**other".to_string()],
        ..Default::default()
      },
      TestInfoFilePlugin {
        name: "dprint-process-plugin".to_string(),
        version: "0.1.0".to_string(),
        url: "https://plugins.dprint.dev/process-0.1.0.json".to_string(),
        file_extensions: vec!["ps".to_string()],
        config_excludes: vec![],
        checksum: Some("test-checksum".to_string()),
        ..Default::default()
      },
    ]
  }
}
