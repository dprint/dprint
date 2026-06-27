use std::collections::HashSet;
use std::collections::VecDeque;

use anyhow::Result;
use dprint_core::plugins::wasm::{self};
use jsonc_parser::cst::CstInputValue;
use jsonc_parser::cst::CstRootNode;

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
    let defaults = compute_default_selections(&latest_plugins, &project_files);

    let mut selected_indexes = if options.non_interactive {
      defaults.iter().enumerate().filter_map(|(i, on)| on.then_some(i)).collect::<Vec<_>>()
    } else {
      // show the pre-selected plugins at the top of the list
      let order = display_order(&defaults);
      let prompt_message = "Select plugins (space to toggle, type to filter, enter to finish):";
      let items = order
        .iter()
        .map(|&i| (defaults[i], plugin_display_text(&latest_plugins[i])))
        .collect::<Vec<_>>();
      let chosen = environment.get_multi_selection(prompt_message, 0, &items)?;
      chosen.into_iter().map(|display_index| order[display_index]).collect::<Vec<_>>()
    };
    // keep the config file in info.json order regardless of the display order
    selected_indexes.sort_unstable();

    let mut selected_plugins = Vec::new();
    for index in selected_indexes {
      let plugin = latest_plugins[index].clone();
      let config = build_plugin_config(&plugin, &project_files);
      selected_plugins.push((plugin, config));
    }
    Some(selected_plugins)
  } else {
    None
  };

  let json_text = match selected_plugins {
    Some(selected_plugins) if !selected_plugins.is_empty() => render_config_file(&selected_plugins),
    // plugin info was available, but nothing was selected
    Some(_) => "{\n  \"excludes\": [],\n  \"plugins\": [\n    // specify plugin urls here\n  ]\n}\n".to_string(),
    // the plugin info couldn't be downloaded
    None => "{\n  \"excludes\": [\n    \"**/*-lock.json\"\n  ],\n  \"plugins\": [\n    // specify plugin urls here\n  ]\n}\n".to_string(),
  };

  Ok(json_text)
}

/// Renders the config file for the selected plugins using jsonc-parser's CST,
/// which handles indentation, commas, and multi-line formatting for us.
fn render_config_file(selected_plugins: &[(InfoFilePluginInfo, serde_json::Value)]) -> String {
  let root = CstRootNode::parse("{\n}", &Default::default()).unwrap();
  let root_obj = root.object_value_or_set();

  for (plugin, config) in selected_plugins {
    if let Some(config_key) = &plugin.config_key
      && !config_key.is_empty()
    {
      let is_empty = config.as_object().is_none_or(|obj| obj.is_empty());
      let prop = root_obj.append(config_key, to_cst_input(config.clone()));
      // keep the brace of an empty block on its own line so there's a spot to add options
      if is_empty && let Some(object) = prop.value().and_then(|value| value.as_object()) {
        object.ensure_multiline();
      }
    }
  }

  let excludes = get_unique_items(
    selected_plugins
      .iter()
      .flat_map(|(plugin, _)| plugin.config_excludes.iter().cloned())
      .collect::<Vec<_>>(),
  );
  let excludes_prop = root_obj.append("excludes", CstInputValue::Array(excludes.iter().cloned().map(CstInputValue::String).collect()));
  if !excludes.is_empty()
    && let Some(array) = excludes_prop.value().and_then(|value| value.as_array())
  {
    array.ensure_multiline();
  }

  let urls = selected_plugins
    .iter()
    .map(|(plugin, _)| {
      if plugin.is_process_plugin() && plugin.checksum.is_some() {
        format!("{}@{}", plugin.url, plugin.checksum.as_ref().unwrap())
      } else {
        plugin.url.clone()
      }
    })
    .collect::<Vec<_>>();
  let plugins_prop = root_obj.append("plugins", CstInputValue::Array(urls.into_iter().map(CstInputValue::String).collect()));
  if let Some(array) = plugins_prop.value().and_then(|value| value.as_array()) {
    array.ensure_multiline();
  }

  format!("{root}\n")
}

/// Converts an owned `serde_json::Value` into the CST's input value type.
fn to_cst_input(value: serde_json::Value) -> CstInputValue {
  use serde_json::Value;
  match value {
    Value::Null => CstInputValue::Null,
    Value::Bool(value) => CstInputValue::Bool(value),
    Value::Number(value) => CstInputValue::Number(value.to_string()),
    Value::String(value) => CstInputValue::String(value),
    Value::Array(values) => CstInputValue::Array(values.into_iter().map(to_cst_input).collect()),
    Value::Object(entries) => CstInputValue::Object(entries.into_iter().map(|(key, value)| (key, to_cst_input(value))).collect()),
  }
}

/// The files found while scanning the current directory, used to decide which
/// plugins to pre-select.
struct ProjectFiles {
  /// Lowercased file extensions without the leading dot (ex. `ts`, `json`).
  extensions: HashSet<String>,
  file_names: HashSet<String>,
}

/// Decides which plugins to pre-select based on the files in the current
/// directory, applying two priority rules in info.json order:
///
/// 1. Each present extension / file name is claimed by the first plugin that
///    matches it; a plugin is pre-selected when it claims at least one. So a
///    later plugin is still selected if it's the first to match some other
///    present extension (ex. one plugin owns `.ts`, a later one owns `.vue`).
/// 2. Two plugins that share a config key are never both selected — the earlier
///    one in the list wins.
///
/// A plugin matches via its own file extensions / file names or via any of its
/// config items (ex. `dprint-plugin-exec` declares no extensions of its own but
/// pre-selects when one of its command's file types is present).
fn compute_default_selections(plugins: &[InfoFilePluginInfo], project_files: &ProjectFiles) -> Vec<bool> {
  let mut claimed_extensions: HashSet<String> = HashSet::new();
  let mut claimed_file_names: HashSet<String> = HashSet::new();
  let mut used_config_keys: HashSet<&str> = HashSet::new();
  let mut selected = vec![false; plugins.len()];

  for (i, plugin) in plugins.iter().enumerate() {
    // present extensions / file names that this plugin matches
    let present_extensions = match_extensions(plugin)
      .into_iter()
      .filter(|ext| project_files.extensions.contains(ext))
      .collect::<Vec<_>>();
    let present_file_names = match_file_names(plugin)
      .into_iter()
      .filter(|name| project_files.file_names.contains(*name))
      .map(|name| name.to_string())
      .collect::<Vec<_>>();

    // select it only if it's the first to match at least one of those
    let claims_unclaimed =
      present_extensions.iter().any(|ext| !claimed_extensions.contains(ext)) || present_file_names.iter().any(|name| !claimed_file_names.contains(name));
    if !claims_unclaimed {
      continue;
    }

    // never select two plugins that share a config key (earlier one wins)
    if let Some(config_key) = config_key(plugin)
      && !used_config_keys.insert(config_key)
    {
      continue;
    }

    selected[i] = true;
    claimed_extensions.extend(present_extensions);
    claimed_file_names.extend(present_file_names);
  }

  selected
}

/// The order plugins are displayed in: the pre-selected ones first (each group
/// in info.json order), so the relevant plugins are at the top of the list.
fn display_order(defaults: &[bool]) -> Vec<usize> {
  let selected = (0..defaults.len()).filter(|&i| defaults[i]);
  let unselected = (0..defaults.len()).filter(|&i| !defaults[i]);
  selected.chain(unselected).collect()
}

/// All the file extensions a plugin matches (its own plus its config items'),
/// lowercased to match the scan.
fn match_extensions(plugin: &InfoFilePluginInfo) -> Vec<String> {
  let mut extensions = plugin.file_extensions.iter().map(|ext| ext.to_lowercase()).collect::<Vec<_>>();
  for item in &plugin.config_items {
    extensions.extend(item.file_extensions.iter().map(|ext| ext.to_lowercase()));
  }
  extensions
}

/// All the file names a plugin matches (its own plus its config items').
fn match_file_names(plugin: &InfoFilePluginInfo) -> Vec<&str> {
  let mut file_names = plugin.file_names.iter().map(String::as_str).collect::<Vec<_>>();
  for item in &plugin.config_items {
    file_names.extend(item.file_names.iter().map(String::as_str));
  }
  file_names
}

/// A plugin's non-empty config key, if it has one.
fn config_key(plugin: &InfoFilePluginInfo) -> Option<&str> {
  plugin.config_key.as_deref().filter(|key| !key.is_empty())
}

/// Whether any of the extensions or file names are present in the current
/// directory. Extensions are matched case-insensitively, while file names are
/// matched exactly because their casing is significant (ex. `Cargo.toml`).
fn matches_project_files(file_extensions: &[String], file_names: &[String], project_files: &ProjectFiles) -> bool {
  file_extensions.iter().any(|ext| project_files.extensions.contains(&ext.to_lowercase()))
    || file_names.iter().any(|name| project_files.file_names.contains(name))
}

/// The text shown for a plugin in the selection list. The supported file
/// extensions are appended so unfamiliar plugins are easier to tell apart.
fn plugin_display_text(plugin: &InfoFilePluginInfo) -> String {
  let extensions = display_extensions(plugin);
  if extensions.is_empty() {
    plugin.name.clone()
  } else {
    let extensions = extensions.iter().map(|ext| format!(".{}", ext)).collect::<Vec<_>>().join(", ");
    format!("{} ({})", plugin.name, extensions)
  }
}

/// The extensions to show beside a plugin's name, falling back to the extensions
/// of its config items when the plugin declares none of its own.
fn display_extensions(plugin: &InfoFilePluginInfo) -> Vec<String> {
  if !plugin.file_extensions.is_empty() {
    return plugin.file_extensions.clone();
  }
  let mut extensions = Vec::new();
  for item in &plugin.config_items {
    for ext in &item.file_extensions {
      if !extensions.contains(ext) {
        extensions.push(ext.clone());
      }
    }
  }
  extensions
}

/// Builds the config block for a selected plugin: starts from its `defaultConfig`
/// and merges in each config item whose files are present in the current
/// directory. Objects merge recursively and arrays are concatenated, so command
/// lists (ex. `dprint-plugin-exec`) accumulate.
///
/// The order keys appear in the output relies on `serde_json`'s `preserve_order`
/// feature (and jsonc-parser's), so config reads in the same order it's declared.
fn build_plugin_config(plugin: &InfoFilePluginInfo, project_files: &ProjectFiles) -> serde_json::Value {
  let mut config = match plugin.default_config.clone() {
    Some(value @ serde_json::Value::Object(_)) => value,
    _ => serde_json::Value::Object(Default::default()),
  };
  for item in &plugin.config_items {
    if matches_project_files(&item.file_extensions, &item.file_names, project_files) {
      deep_merge(&mut config, item.config.clone());
    }
  }
  config
}

/// Merges `other` into `base`: objects merge recursively, arrays are
/// concatenated, and anything else overwrites.
fn deep_merge(base: &mut serde_json::Value, other: serde_json::Value) {
  use serde_json::Value;
  match other {
    Value::Object(other_map) if base.is_object() => {
      let base_map = base.as_object_mut().unwrap();
      for (key, value) in other_map {
        match base_map.get_mut(&key) {
          Some(existing) => deep_merge(existing, value),
          None => {
            base_map.insert(key, value);
          }
        }
      }
    }
    Value::Array(mut other_items) if base.is_array() => {
      base.as_array_mut().unwrap().append(&mut other_items);
    }
    other => *base = other,
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
  use crate::environment::TestInfoFileConfigItem;
  use crate::environment::TestInfoFileMatch;
  use crate::environment::TestInfoFilePlugin;
  use crate::plugins::InfoFileConfigItem;
  use pretty_assertions::assert_eq;

  fn exec_info_plugin() -> TestInfoFilePlugin {
    TestInfoFilePlugin {
      name: "dprint-plugin-exec".to_string(),
      version: "0.5.0".to_string(),
      url: "https://plugins.dprint.dev/exec-0.5.0.json".to_string(),
      config_key: Some("exec".to_string()),
      file_extensions: vec![],
      config_excludes: vec![],
      checksum: Some("checksum".to_string()),
      // ${configDir} is an exec-plugin runtime token, emitted verbatim (not expanded here)
      default_config: Some(serde_json::json!({ "cwd": "${configDir}" })),
      config_items: vec![
        TestInfoFileConfigItem {
          file_match: TestInfoFileMatch {
            file_extensions: vec!["rs".to_string()],
            file_names: vec![],
          },
          config: serde_json::json!({ "commands": [{ "command": "rustfmt", "exts": ["rs"] }] }),
        },
        TestInfoFileConfigItem {
          file_match: TestInfoFileMatch {
            file_extensions: vec!["go".to_string()],
            file_names: vec![],
          },
          config: serde_json::json!({ "commands": [{ "command": "gofmt", "exts": ["go"] }] }),
        },
      ],
      ..Default::default()
    }
  }

  fn info_plugin_with_extensions(file_extensions: Vec<&str>, config_item_extensions: Vec<Vec<&str>>) -> InfoFilePluginInfo {
    InfoFilePluginInfo {
      name: "dprint-plugin-exec".to_string(),
      version: "0.5.0".to_string(),
      url: "https://plugins.dprint.dev/exec-0.5.0.json".to_string(),
      config_key: Some("exec".to_string()),
      file_extensions: file_extensions.into_iter().map(String::from).collect(),
      file_names: vec![],
      config_excludes: vec![],
      checksum: None,
      default_config: None,
      config_items: config_item_extensions
        .into_iter()
        .map(|extensions| InfoFileConfigItem {
          file_extensions: extensions.into_iter().map(String::from).collect(),
          file_names: vec![],
          config: serde_json::Value::Object(Default::default()),
        })
        .collect(),
    }
  }

  #[test]
  fn plugin_display_text_falls_back_to_config_item_extensions() {
    // a plugin's own extensions take precedence
    let plugin = info_plugin_with_extensions(vec!["ts", "tsx"], vec![vec!["rs"]]);
    assert_eq!(plugin_display_text(&plugin), "dprint-plugin-exec (.ts, .tsx)");
    // otherwise the extensions are derived from the config items, deduplicated in order
    let plugin = info_plugin_with_extensions(vec![], vec![vec!["rs"], vec!["go", "rs"]]);
    assert_eq!(plugin_display_text(&plugin), "dprint-plugin-exec (.rs, .go)");
    // nothing to show -> just the name
    let plugin = info_plugin_with_extensions(vec![], vec![]);
    assert_eq!(plugin_display_text(&plugin), "dprint-plugin-exec");
  }

  #[test]
  fn should_scaffold_config_from_file_name_match() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info.add_plugin(TestInfoFilePlugin {
          name: "dprint-plugin-exec".to_string(),
          version: "0.5.0".to_string(),
          url: "https://plugins.dprint.dev/exec-0.5.0.json".to_string(),
          config_key: Some("exec".to_string()),
          file_extensions: vec![],
          config_excludes: vec![],
          checksum: Some("checksum".to_string()),
          config_items: vec![TestInfoFileConfigItem {
            file_match: TestInfoFileMatch {
              file_extensions: vec![],
              file_names: vec!["Makefile".to_string()],
            },
            config: serde_json::json!({ "commands": [{ "command": "make", "exts": [] }] }),
          }],
          ..Default::default()
        });
      })
      .write_file("/Makefile", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      assert_eq!(
        text,
        r#"{
  "exec": {
    "commands": [
      {
        "command": "make",
        "exts": []
      }
    ]
  },
  "excludes": [],
  "plugins": [
    "https://plugins.dprint.dev/exec-0.5.0.json@checksum"
  ]
}
"#
      );
      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_let_config_item_override_default_config() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info.add_plugin(TestInfoFilePlugin {
          name: "dprint-plugin-exec".to_string(),
          version: "0.5.0".to_string(),
          url: "https://plugins.dprint.dev/exec-0.5.0.json".to_string(),
          config_key: Some("exec".to_string()),
          file_extensions: vec![],
          config_excludes: vec![],
          checksum: Some("checksum".to_string()),
          default_config: Some(serde_json::json!({ "cwd": "default", "indentWidth": 2 })),
          config_items: vec![TestInfoFileConfigItem {
            file_match: TestInfoFileMatch {
              file_extensions: vec!["rs".to_string()],
              file_names: vec![],
            },
            config: serde_json::json!({ "cwd": "${configDir}", "commands": [{ "command": "rustfmt", "exts": ["rs"] }] }),
          }],
          ..Default::default()
        });
      })
      .write_file("/main.rs", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      // the config item overrides cwd, keeps indentWidth, and appends commands
      assert_eq!(
        text,
        r#"{
  "exec": {
    "cwd": "${configDir}",
    "indentWidth": 2,
    "commands": [
      {
        "command": "rustfmt",
        "exts": ["rs"]
      }
    ]
  },
  "excludes": [],
  "plugins": [
    "https://plugins.dprint.dev/exec-0.5.0.json@checksum"
  ]
}
"#
      );
      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_auto_select_exec_non_interactive() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info.add_plugin(exec_info_plugin());
      })
      .write_file("/main.rs", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, GetInitConfigFileTextOptions { non_interactive: true })
        .await
        .unwrap();
      // exec is auto-selected by the .rs file with no prompt
      assert!(text.contains("\"command\": \"rustfmt\""), "{text}");
      assert!(text.contains("exec-0.5.0.json@checksum"), "{text}");
      assert_eq!(environment.take_stderr_messages(), Vec::<String>::new());
    });
  }

  #[test]
  fn should_scaffold_exec_config_from_matched_file() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info.add_plugin(exec_info_plugin());
      })
      .write_file("/main.rs", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      // exec is pre-selected by the .rs file; cwd comes from defaultConfig and the
      // rustfmt command from the matching config item (gofmt is not added)
      assert_eq!(
        text,
        r#"{
  "exec": {
    "cwd": "${configDir}",
    "commands": [
      {
        "command": "rustfmt",
        "exts": ["rs"]
      }
    ]
  },
  "excludes": [],
  "plugins": [
    "https://plugins.dprint.dev/exec-0.5.0.json@checksum"
  ]
}
"#
      );
      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_concatenate_config_items_in_info_order() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info.add_plugin(exec_info_plugin());
      })
      .write_file("/main.rs", "")
      .write_file("/lib.go", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      // both commands are appended in info.json order
      assert_eq!(
        text,
        r#"{
  "exec": {
    "cwd": "${configDir}",
    "commands": [
      {
        "command": "rustfmt",
        "exts": ["rs"]
      },
      {
        "command": "gofmt",
        "exts": ["go"]
      }
    ]
  },
  "excludes": [],
  "plugins": [
    "https://plugins.dprint.dev/exec-0.5.0.json@checksum"
  ]
}
"#
      );
      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_apply_only_default_config_when_no_items_match() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info.add_plugin(exec_info_plugin());
      })
      .build();
    // manually select exec even though no matching files are present
    environment.set_multi_selection_result(vec![0]);
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      assert_eq!(
        text,
        r#"{
  "exec": {
    "cwd": "${configDir}"
  },
  "excludes": [],
  "plugins": [
    "https://plugins.dprint.dev/exec-0.5.0.json@checksum"
  ]
}
"#
      );
      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn deep_merge_combines_objects_and_arrays() {
    let mut base = serde_json::json!({ "a": 1, "list": [1, 2], "nested": { "x": 1 } });
    deep_merge(&mut base, serde_json::json!({ "a": 2, "list": [3], "nested": { "y": 2 }, "b": 3 }));
    assert_eq!(
      base,
      // scalars override, arrays concatenate, objects merge, new keys insert
      serde_json::json!({ "a": 2, "list": [1, 2, 3], "nested": { "x": 1, "y": 2 }, "b": 3 })
    );
  }

  fn wasm_plugin(name: &str, config_key: &str, extensions: &[&str]) -> TestInfoFilePlugin {
    TestInfoFilePlugin {
      name: name.to_string(),
      version: "1.0.0".to_string(),
      url: format!("https://plugins.dprint.dev/{}-1.0.0.wasm", name),
      config_key: Some(config_key.to_string()),
      file_extensions: extensions.iter().map(|e| e.to_string()).collect(),
      config_excludes: vec![],
      ..Default::default()
    }
  }

  #[test]
  fn display_order_lists_selected_first() {
    assert_eq!(display_order(&[false, true, false, true]), vec![1, 3, 0, 2]);
    assert_eq!(display_order(&[true, true]), vec![0, 1]);
    assert_eq!(display_order(&[false, false, false]), vec![0, 1, 2]);
    assert_eq!(display_order(&[]), Vec::<usize>::new());
  }

  #[test]
  fn should_select_only_the_first_plugin_for_a_shared_extension() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info.add_plugin(wasm_plugin("a", "a", &["ts"])).add_plugin(wasm_plugin("b", "b", &["ts"]));
      })
      .write_file("/file.ts", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      // only the first plugin claims `ts`
      assert!(text.contains("a-1.0.0.wasm"), "{text}");
      assert!(!text.contains("b-1.0.0.wasm"), "{text}");
      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_select_both_plugins_on_partial_extension_overlap() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        // `a` owns `ts`; `b` shares `ts` but also owns `vue`, so it's still selected
        info
          .add_plugin(wasm_plugin("a", "a", &["ts"]))
          .add_plugin(wasm_plugin("b", "b", &["ts", "vue"]));
      })
      .write_file("/file.ts", "")
      .write_file("/file.vue", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      assert!(text.contains("a-1.0.0.wasm"), "{text}");
      assert!(text.contains("b-1.0.0.wasm"), "{text}");
      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_not_select_two_plugins_with_the_same_config_key() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        // both use config key "markup"; `b` would otherwise be selected for `vue`
        info
          .add_plugin(wasm_plugin("a", "markup", &["ts"]))
          .add_plugin(wasm_plugin("b", "markup", &["ts", "vue"]));
      })
      .write_file("/file.ts", "")
      .write_file("/file.vue", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      assert!(text.contains("a-1.0.0.wasm"), "{text}");
      assert!(!text.contains("b-1.0.0.wasm"), "{text}");
      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_map_picker_selection_back_when_display_is_reordered() {
    // `b` (index 1) is the sole pre-selected plugin, so it's shown first; selecting
    // both from the reordered list must still map back and output in info.json order
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info.add_plugin(wasm_plugin("a", "a", &["js"])).add_plugin(wasm_plugin("b", "b", &["ts"]));
      })
      .write_file("/file.ts", "")
      .build();
    environment.set_multi_selection_result(vec![0, 1]); // both items, in display order
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      let a_pos = text.find("a-1.0.0.wasm").expect("a present");
      let b_pos = text.find("b-1.0.0.wasm").expect("b present");
      // output stays in info.json order (a before b) even though b was displayed first
      assert!(a_pos < b_pos, "{text}");
      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_select_keyless_plugins_for_distinct_extensions() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info
          .add_plugin(TestInfoFilePlugin {
            config_key: None,
            ..wasm_plugin("a", "", &["ts"])
          })
          .add_plugin(TestInfoFilePlugin {
            config_key: None,
            ..wasm_plugin("b", "", &["vue"])
          });
      })
      .write_file("/file.ts", "")
      .write_file("/file.vue", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      // plugins without a config key never collide on it
      assert!(text.contains("a-1.0.0.wasm"), "{text}");
      assert!(text.contains("b-1.0.0.wasm"), "{text}");
      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_not_select_plugins_with_shared_config_key_matched_via_config_items() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info.add_plugin(wasm_plugin("a", "shared", &["ts"])).add_plugin(TestInfoFilePlugin {
          name: "b".to_string(),
          version: "1.0.0".to_string(),
          url: "https://plugins.dprint.dev/b-1.0.0.wasm".to_string(),
          config_key: Some("shared".to_string()),
          file_extensions: vec![],
          config_excludes: vec![],
          config_items: vec![TestInfoFileConfigItem {
            file_match: TestInfoFileMatch {
              file_extensions: vec!["rs".to_string()],
              file_names: vec![],
            },
            config: serde_json::json!({}),
          }],
          ..Default::default()
        });
      })
      .write_file("/file.ts", "")
      .write_file("/file.rs", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      // `b` matches `.rs` via a config item but shares the "shared" key with `a`
      assert!(text.contains("a-1.0.0.wasm"), "{text}");
      assert!(!text.contains("b-1.0.0.wasm"), "{text}");
      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

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
