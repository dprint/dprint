use anyhow::Result;
use dprint_core::plugins::wasm::{self};

use crate::environment::Environment;
use crate::plugins::read_info_file;

pub async fn get_init_config_file_text(environment: &impl Environment) -> Result<String> {
  let info = match read_info_file(environment).await {
    Ok(info) => {
      // ok to only check wasm here because the configuration file is only ever initialized with wasm plugins
      if info.plugin_system_schema_version != wasm::PLUGIN_SYSTEM_SCHEMA_VERSION {
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
    let prompt_message = "Select plugins (use the spacebar to select/deselect and then press enter when finished):";
    let plugin_indexes = environment.get_multi_selection(
      prompt_message,
      0,
      &latest_plugins
        .iter()
        .map(|x| (x.selected && !x.is_process_plugin(), String::from(&x.name)))
        .collect::<Vec<_>>(),
    )?;
    let mut selected_plugins = Vec::new();
    for index in plugin_indexes {
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
      if let Some(config_key) = &plugin.config_key {
        if !config_key.is_empty() {
          json_text.push_str(&format!("  \"{}\": {{\n", config_key));
          json_text.push_str("  },\n");
        }
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
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        for plugin in get_multi_plugins_config() {
          info.add_plugin(plugin);
        }
      })
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment).await.unwrap();
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
      let text = get_init_config_file_text(&environment).await.unwrap();
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
      let text = get_init_config_file_text(&environment).await.unwrap();
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
      let text = get_init_config_file_text(&environment).await.unwrap();
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
      let text = get_init_config_file_text(&environment).await.unwrap();
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
      let text = get_init_config_file_text(&environment).await.unwrap();
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
      let text = get_init_config_file_text(&environment).await.unwrap();
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
      let text = get_init_config_file_text(&environment).await.unwrap();
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
    vec!["Select plugins (use the spacebar to select/deselect and then press enter when finished):"]
  }

  fn get_multi_plugins_config() -> Vec<TestInfoFilePlugin> {
    vec![
      TestInfoFilePlugin {
        name: "dprint-plugin-typescript".to_string(),
        version: "0.17.2".to_string(),
        selected: Some(true),
        url: "https://plugins.dprint.dev/typescript-0.17.2.wasm".to_string(),
        config_key: Some("typescript".to_string()),
        file_extensions: vec!["ts".to_string(), "tsx".to_string()],
        config_excludes: vec!["**/something".to_string()],
        ..Default::default()
      },
      TestInfoFilePlugin {
        name: "dprint-plugin-jsonc".to_string(),
        version: "0.2.3".to_string(),
        selected: Some(true),
        url: "https://plugins.dprint.dev/json-0.2.3.wasm".to_string(),
        config_key: Some("json".to_string()),
        file_extensions: vec!["json".to_string()],
        config_excludes: vec!["**/*-asdf.json".to_string()],
        ..Default::default()
      },
      TestInfoFilePlugin {
        name: "dprint-plugin-final".to_string(),
        version: "0.1.2".to_string(),
        selected: Some(false),
        url: "https://plugins.dprint.dev/final-0.1.2.wasm".to_string(),
        file_names: Some(vec!["Cargo.toml".to_string()]),
        file_extensions: vec!["tsx".to_string(), "rs".to_string()],
        config_excludes: vec!["**/something".to_string(), "**other".to_string()],
        ..Default::default()
      },
      TestInfoFilePlugin {
        name: "dprint-process-plugin".to_string(),
        version: "0.1.0".to_string(),
        selected: Some(true), // should ignore this even though it's selected
        url: "https://plugins.dprint.dev/process-0.1.0.json".to_string(),
        file_extensions: vec!["ps".to_string()],
        config_excludes: vec![],
        checksum: Some("test-checksum".to_string()),
        ..Default::default()
      },
    ]
  }
}
