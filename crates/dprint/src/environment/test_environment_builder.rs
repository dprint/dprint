use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use super::{Environment, TestEnvironment};
use crate::test_helpers;

pub struct TestConfigFileBuilder {
  environment: TestEnvironment,
  incremental: Option<bool>,
  includes: Option<Vec<String>>,
  excludes: Option<Vec<String>>,
  plugins: Option<Vec<String>>,
  sections: HashMap<String, String>,
}

impl TestConfigFileBuilder {
  fn new(environment: TestEnvironment) -> Self {
    TestConfigFileBuilder {
      environment,
      incremental: None,
      includes: None,
      excludes: None,
      plugins: None,
      sections: HashMap::new(),
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
      let plugins_text = plugins.iter().map(|name| format!("\"{}\"", name)).collect::<Vec<_>>().join(",\n");
      parts.push(format!("\"plugins\": [\n{}\n]", plugins_text))
    }
    if let Some(includes) = self.includes.as_ref() {
      let text = includes.iter().map(|v| format!("\"{}\"", v)).collect::<Vec<_>>().join(",\n");
      parts.push(format!("\"includes\": [\n{}\n]", text))
    }
    if let Some(excludes) = self.excludes.as_ref() {
      let text = excludes.iter().map(|v| format!("\"{}\"", v)).collect::<Vec<_>>().join(",\n");
      parts.push(format!("\"excludes\": [\n{}\n]", text))
    }
    format!("{{\n{}\n}}", parts.join(",\n"))
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

  pub fn add_config_section(&mut self, name: &str, text: &str) -> &mut Self {
    self.sections.insert(name.to_string(), text.to_string());
    self
  }

  pub fn add_remote_process_plugin(&mut self) -> &mut Self {
    self.add_remote_process_plugin_with_checksum(&test_helpers::get_test_process_plugin_checksum(&self.environment))
  }

  pub fn add_remote_process_plugin_with_checksum(&mut self, checksum: &str) -> &mut Self {
    self.add_plugin(&format!("https://plugins.dprint.dev/test-process.exe-plugin@{}", checksum,))
  }

  pub fn ensure_plugins_section(&mut self) -> &mut Self {
    if self.plugins.is_none() {
      self.plugins = Some(Vec::new());
    }
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

pub struct TestEnvironmentBuilder {
  environment: TestEnvironment,
  config_files: HashMap<String, TestConfigFileBuilder>,
}

impl TestEnvironmentBuilder {
  pub fn new() -> Self {
    Self {
      environment: TestEnvironment::new(),
      config_files: HashMap::new(),
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
    self
      .environment
      .add_remote_file("https://plugins.dprint.dev/test-plugin.wasm", test_helpers::WASM_PLUGIN_BYTES);
    self
  }

  pub fn with_default_config(&mut self, func: impl FnMut(&mut TestConfigFileBuilder)) -> &mut Self {
    self.with_local_config("./dprint.json", func)
  }

  pub fn with_local_config(&mut self, file_path: impl AsRef<Path>, func: impl FnMut(&mut TestConfigFileBuilder)) -> &mut Self {
    let config_file_text = self.with_config_get_text(&file_path.as_ref().to_string_lossy(), func);
    self.write_file(file_path, &config_file_text)
  }

  pub fn with_remote_config(&mut self, url: &str, func: impl FnMut(&mut TestConfigFileBuilder)) -> &mut Self {
    let config_file_text = self.with_config_get_text(url, func);
    self.add_remote_file(url, &config_file_text)
  }

  fn with_config_get_text(&mut self, key: &str, mut func: impl FnMut(&mut TestConfigFileBuilder)) -> String {
    let config_file = self.config_files.entry(key.to_string()).or_insert_with({
      let environment = self.environment.clone();
      || TestConfigFileBuilder::new(environment)
    });
    func(config_file);
    config_file.to_string()
  }

  pub fn write_file(&mut self, file_path: impl AsRef<Path>, text: &str) -> &mut Self {
    self.environment.write_file(file_path, text).unwrap();
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
    self
      .environment
      .write_file_bytes(&PathBuf::from("/plugins/test-plugin.wasm"), test_helpers::WASM_PLUGIN_BYTES)
      .unwrap();
    self
  }

  pub fn add_remote_process_plugin(&mut self) -> &mut Self {
    let buf: Vec<u8> = Vec::new();
    let w = std::io::Cursor::new(buf);
    let mut zip = zip::ZipWriter::new(w);
    let options = zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip
      .start_file(
        if cfg!(target_os = "windows") {
          "test-process-plugin.exe"
        } else {
          "test-process-plugin"
        },
        options,
      )
      .unwrap();
    zip.write(test_helpers::PROCESS_PLUGIN_EXE_BYTES).unwrap();
    let result = zip.finish().unwrap().into_inner();
    let zip_file_checksum = dprint_cli_core::checksums::get_sha256_checksum(&result);
    self
      .environment
      .add_remote_file_bytes("https://github.com/dprint/test-process-plugin/releases/0.1.0/test-process-plugin.zip", result);
    self.write_process_plugin_file(&zip_file_checksum);
    self
  }

  pub fn write_process_plugin_file(&mut self, zip_checksum: &str) -> &mut Self {
    self.environment.add_remote_file_bytes(
      "https://plugins.dprint.dev/test-process.exe-plugin",
      format!(
        r#"{{
    "schemaVersion": 1,
    "name": "test-process-plugin",
    "version": "0.1.0",
    "windows-x86_64": {{
        "reference": "https://github.com/dprint/test-process-plugin/releases/0.1.0/test-process-plugin.zip",
        "checksum": "{0}"
    }},
    "linux-x86_64": {{
        "reference": "https://github.com/dprint/test-process-plugin/releases/0.1.0/test-process-plugin.zip",
        "checksum": "{0}"
    }},
    "mac-x86_64": {{
        "reference": "https://github.com/dprint/test-process-plugin/releases/0.1.0/test-process-plugin.zip",
        "checksum": "{0}"
    }}
}}"#,
        zip_checksum
      )
      .into_bytes(),
    );
    self
  }
}
