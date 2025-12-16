use indexmap::IndexMap;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

use super::Environment;
use super::TestEnvironment;
use crate::test_helpers;
use crate::test_helpers::TestProcessPluginFile;
use crate::test_helpers::WASM_PLUGIN_0_1_0_BYTES;
use crate::utils::get_sha256_checksum;

pub struct TestConfigFileBuilder {
  environment: TestEnvironment,
  incremental: Option<bool>,
  includes: Option<Vec<String>>,
  excludes: Option<Vec<String>>,
  plugins: Option<Vec<String>>,
  sections: IndexMap<String, String>,
}

impl TestConfigFileBuilder {
  pub fn new(environment: TestEnvironment) -> Self {
    TestConfigFileBuilder {
      environment,
      incremental: None,
      includes: None,
      excludes: None,
      plugins: None,
      sections: Default::default(),
    }
  }

  pub fn to_string(&self) -> String {
    let mut parts = Vec::new();
    for (key, value) in self.sections.iter() {
      parts.push(format!("\"{}\": {}", key, value));
    }
    if let Some(incremental) = self.incremental.as_ref() {
      parts.push(format!(r#""incremental": {}"#, incremental));
    }
    // todo: reduce code duplication... was lazy
    if let Some(plugins) = self.plugins.as_ref() {
      let plugins_text = plugins.iter().map(|name| format!("  \"{}\"", name)).collect::<Vec<_>>().join(",\n");
      parts.push(format!("\"plugins\": [\n{}\n]", plugins_text))
    }
    if let Some(includes) = self.includes.as_ref() {
      let text = includes.iter().map(|v| format!("  \"{}\"", v)).collect::<Vec<_>>().join(",\n");
      parts.push(format!("\"includes\": [\n{}\n]", text))
    }
    if let Some(excludes) = self.excludes.as_ref() {
      let text = excludes.iter().map(|v| format!("  \"{}\"", v)).collect::<Vec<_>>().join(",\n");
      parts.push(format!("\"excludes\": [\n{}\n]", text))
    }
    format!(
      "{{\n{}\n}}",
      parts.join(",\n").lines().map(|l| format!("  {}", l)).collect::<Vec<_>>().join("\n")
    )
  }

  pub fn set_incremental(&mut self, value: bool) -> &mut Self {
    self.incremental = Some(value);
    self
  }

  pub fn add_local_wasm_plugin(&mut self) -> &mut Self {
    self.add_plugin("/plugins/test-plugin.wasm")
  }

  pub fn add_remote_wasm_plugin(&mut self) -> &mut Self {
    self.add_plugin("https://plugins.dprint.dev/test-plugin.wasm")
  }

  pub fn add_remote_wasm_plugin_with_checksum(&mut self, checksum: &str) -> &mut Self {
    self.add_plugin(&format!("https://plugins.dprint.dev/test-plugin.wasm@{}", checksum))
  }

  /// This is a v3 old Wasm plugin.
  pub fn add_remote_wasm_plugin_0_1_0(&mut self) -> &mut Self {
    self.add_plugin("https://plugins.dprint.dev/test-plugin-0.1.0.wasm")
  }

  pub fn add_remote_wasm_plugin_0_1_0_with_checksum(&mut self) -> &mut Self {
    self.add_plugin(&format!(
      "https://plugins.dprint.dev/test-plugin-0.1.0.wasm@{}",
      &crate::utils::get_sha256_checksum(&WASM_PLUGIN_0_1_0_BYTES)
    ))
  }

  pub fn add_config_section(&mut self, name: &str, text: &str) -> &mut Self {
    self.sections.insert(name.to_string(), text.to_string());
    self
  }

  pub fn add_remote_process_plugin(&mut self) -> &mut Self {
    // get the process plugin file and check its checksum
    let remote_file_text = self
      .environment
      .get_remote_file("https://plugins.dprint.dev/test-process.json")
      .unwrap()
      .unwrap();
    let checksum = get_sha256_checksum(&remote_file_text);
    self.add_remote_process_plugin_with_checksum(&checksum)
  }

  pub fn add_remote_process_plugin_with_checksum(&mut self, checksum: &str) -> &mut Self {
    let url = "https://plugins.dprint.dev/test-process.json";
    if checksum.is_empty() {
      self.add_plugin(url)
    } else {
      self.add_plugin(&format!("{}@{}", url, checksum))
    }
  }

  pub fn ensure_plugins_section(&mut self) -> &mut Self {
    if self.plugins.is_none() {
      self.plugins = Some(Vec::new());
    }
    self
  }

  pub fn clear_plugins(&mut self) -> &mut Self {
    self.plugins = None;
    self
  }

  pub fn add_plugin(&mut self, plugin: &str) -> &mut Self {
    let mut plugins = self.plugins.take().unwrap_or_else(Vec::new);
    plugins.push(plugin.to_string());
    self.plugins = Some(plugins);
    self
  }

  pub fn add_includes(&mut self, includes_item: &str) -> &mut Self {
    let mut includes = self.includes.take().unwrap_or_else(Vec::new);
    includes.push(includes_item.to_string());
    self.includes = Some(includes);
    self
  }

  pub fn add_excludes(&mut self, excludes_item: &str) -> &mut Self {
    let mut excludes = self.excludes.take().unwrap_or_else(Vec::new);
    excludes.push(excludes_item.to_string());
    self.excludes = Some(excludes);
    self
  }
}

#[derive(Default)]
pub struct TestInfoFileBuilder {
  plugin_schema_version: Option<usize>,
  plugins: Vec<TestInfoFilePlugin>,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TestInfoFilePlugin {
  pub name: String,
  pub version: String,
  pub url: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub selected: Option<bool>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub file_names: Option<Vec<String>>,
  pub file_extensions: Vec<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub config_key: Option<String>,
  pub config_excludes: Vec<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub checksum: Option<String>,
}

impl TestInfoFileBuilder {
  pub fn add_plugin(&mut self, plugin: TestInfoFilePlugin) -> &mut Self {
    self.plugins.push(plugin);
    self
  }

  pub fn set_plugin_schema_version(&mut self, version: usize) -> &mut Self {
    self.plugin_schema_version = Some(version);
    self
  }

  pub fn to_string(&self) -> String {
    let mut parts = Vec::new();
    parts.push("\"schemaVersion\": 4".to_string());
    parts.push(format!("\"pluginSystemSchemaVersion\": {}", self.plugin_schema_version.unwrap_or(4)));
    let plugins_text = serde_json::to_string_pretty(&self.plugins).unwrap();
    parts.push(format!("\"latest\": {}", plugins_text));
    format!("{{\n{}\n}}", parts.join(",\n"))
  }
}

pub struct TestEnvironmentBuilder {
  environment: TestEnvironment,
  config_files: HashMap<String, TestConfigFileBuilder>,
  info_file: Option<TestInfoFileBuilder>,
}

impl TestEnvironmentBuilder {
  pub fn new() -> Self {
    Self {
      environment: TestEnvironment::new(),
      config_files: HashMap::new(),
      info_file: None,
    }
  }

  pub fn with_remote_wasm_plugin() -> TestEnvironmentBuilder {
    let mut builder = TestEnvironmentBuilder::new();
    builder.add_remote_wasm_plugin();
    builder
  }

  pub fn with_initialized_remote_wasm_plugin() -> TestEnvironmentBuilder {
    let mut builder = TestEnvironmentBuilder::new();
    builder
      .add_remote_wasm_plugin()
      .with_default_config(|config_file| {
        config_file.add_remote_wasm_plugin();
      })
      .initialize();
    builder
  }

  pub fn with_remote_process_plugin() -> TestEnvironmentBuilder {
    let mut builder = TestEnvironmentBuilder::new();
    builder.add_remote_process_plugin();
    builder
  }

  pub fn with_initialized_remote_process_plugin() -> TestEnvironmentBuilder {
    let mut builder = TestEnvironmentBuilder::new();
    builder
      .add_remote_process_plugin()
      .with_default_config(|config_file| {
        config_file.add_remote_process_plugin();
      })
      .initialize();
    builder
  }

  pub fn with_initialized_remote_wasm_and_process_plugin() -> TestEnvironmentBuilder {
    let mut builder = TestEnvironmentBuilder::new();
    builder
      .add_remote_process_plugin()
      .add_remote_wasm_plugin()
      .with_default_config(|config_file| {
        config_file.add_remote_wasm_plugin().add_remote_process_plugin();
      })
      .initialize();
    builder
  }

  pub fn initialize(&mut self) -> &mut Self {
    test_helpers::run_test_cli(vec!["license"], &self.environment).unwrap(); // cause initialization
    self.environment.clear_logs();
    self
  }

  pub fn build(&mut self) -> TestEnvironment {
    self.environment.clone()
  }

  pub fn add_remote_wasm_plugin(&mut self) -> &mut Self {
    self.add_remote_wasm_plugin_at_url("https://plugins.dprint.dev/test-plugin.wasm");
    self
  }

  pub fn add_remote_wasm_0_1_0_plugin(&mut self) -> &mut Self {
    self
      .environment
      .add_remote_file("https://plugins.dprint.dev/test-plugin-0.1.0.wasm", test_helpers::WASM_PLUGIN_0_1_0_BYTES);
    self
  }

  pub fn add_remote_wasm_plugin_at_url(&mut self, url: &str) -> &mut Self {
    self.environment.add_remote_file(url, test_helpers::WASM_PLUGIN_BYTES);
    self
  }

  pub fn with_default_config(&mut self, func: impl FnMut(&mut TestConfigFileBuilder)) -> &mut Self {
    self.with_local_config("/dprint.json", func)
  }

  pub fn with_local_config(&mut self, file_path: impl AsRef<Path>, func: impl FnMut(&mut TestConfigFileBuilder)) -> &mut Self {
    let config_file_text = self.with_config_get_text(&file_path.as_ref().to_string_lossy(), func);
    self.write_file(file_path, &config_file_text)
  }

  pub fn with_remote_config(&mut self, url: &str, func: impl FnMut(&mut TestConfigFileBuilder)) -> &mut Self {
    let config_file_text = self.with_config_get_text(url, func);
    self.add_remote_file(url, &config_file_text)
  }

  pub fn with_global_config(&mut self, func: impl FnMut(&mut TestConfigFileBuilder)) -> &mut Self {
    // Set up the global config directory
    let global_config_dir = "/global-config";
    self.environment.set_env_var("DPRINT_CONFIG_DIR", Some(global_config_dir));

    // Create the global config file
    let config_file_text = self.with_config_get_text("__global_config__", func);
    let global_config_path = format!("{}/dprint.json", global_config_dir);
    self.write_file(&global_config_path, &config_file_text)
  }

  fn with_config_get_text(&mut self, key: &str, mut func: impl FnMut(&mut TestConfigFileBuilder)) -> String {
    let config_file = self.config_files.entry(key.to_string()).or_insert_with({
      let environment = self.environment.clone();
      || TestConfigFileBuilder::new(environment)
    });
    func(config_file);
    config_file.to_string()
  }

  pub fn with_info_file(&mut self, mut func: impl FnMut(&mut TestInfoFileBuilder)) -> &mut Self {
    if self.info_file.is_none() {
      self.info_file = Some(Default::default());
    }
    let info_file_builder = self.info_file.as_mut().unwrap();
    func(info_file_builder);
    self
      .environment
      .add_remote_file_bytes("https://plugins.dprint.dev/info.json", Vec::from(info_file_builder.to_string().as_bytes()));
    self
  }

  pub fn write_file(&mut self, file_path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> &mut Self {
    let file_path = self.environment.clean_path(file_path);
    if let Some(parent) = file_path.parent() {
      self.environment.mk_dir_all(parent).unwrap();
    }
    self.environment.write_file_bytes(file_path, bytes.as_ref()).unwrap();
    self
  }

  pub fn add_staged_file(&mut self, file_path: impl AsRef<Path>) -> &mut Self {
    self.environment.set_staged_file(file_path);
    self
  }

  pub fn add_remote_file(&mut self, path: &str, text: &str) -> &mut Self {
    self.environment.add_remote_file_bytes(path, text.to_string().into_bytes());
    self
  }

  pub fn set_cwd(&mut self, dir_path: &str) -> &mut Self {
    self.environment.set_cwd(dir_path);
    self
  }

  pub fn add_local_wasm_plugin(&mut self) -> &mut Self {
    self.write_file("/plugins/test-plugin.wasm", test_helpers::WASM_PLUGIN_BYTES)
  }

  pub fn add_remote_process_plugin(&mut self) -> &mut Self {
    self.add_remote_process_plugin_at_url("https://plugins.dprint.dev/test-process.json", &TestProcessPluginFile::default())
  }

  pub fn add_remote_process_plugin_at_url(&mut self, url: &str, file_text: &TestProcessPluginFile) -> &mut Self {
    let zip_bytes = &test_helpers::PROCESS_PLUGIN_ZIP_BYTES;
    self.environment.add_remote_file_bytes(
      "https://github.com/dprint/test-process-plugin/releases/0.1.0/test-process-plugin.zip",
      zip_bytes.to_vec(),
    );
    self.environment.add_remote_file_bytes(url, file_text.text().to_string().into_bytes());
    self
  }
}
