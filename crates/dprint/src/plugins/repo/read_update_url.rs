use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use jsonc_parser::JsonValue;
use jsonc_parser::parse_to_value;
use url::Url;

use crate::environment::UrlDownloader;
use crate::plugins::PluginSourceReference;
use crate::utils::PathSource;

const SCHEMA_VERSION: u8 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginUpdateUrlInfo {
  pub url: String,
  pub version: String,
  pub checksum: Option<String>,
}

impl PluginUpdateUrlInfo {
  pub fn is_wasm(&self) -> bool {
    self.url.to_lowercase().ends_with(".wasm")
  }

  pub fn full_url(&self) -> String {
    if let Some(checksum) = &self.checksum {
      return format!("{}@{}", self.url, checksum);
    }
    self.url.to_string()
  }

  pub fn as_source_reference(&self) -> Result<PluginSourceReference> {
    Ok(PluginSourceReference {
      path_source: PathSource::new_remote(Url::parse(&self.url)?),
      checksum: self.checksum.clone(),
    })
  }
}

pub async fn read_update_url(downloader: &impl UrlDownloader, url: &str) -> Result<Option<PluginUpdateUrlInfo>> {
  let info_bytes = match downloader.download_file(url).await? {
    Some(info_bytes) => info_bytes,
    None => return Ok(None),
  };
  let info_text = String::from_utf8(info_bytes.to_vec())?;
  let json_value = parse_to_value(&info_text, &Default::default())?;
  let mut obj = match json_value {
    Some(JsonValue::Object(obj)) => obj,
    _ => bail!("Expected object in root element."),
  };

  // check schema version
  let schema_version = match obj.take_number("schemaVersion") {
    Some(value) => value.parse::<u32>()?,
    _ => bail!("Could not find schema version."),
  };
  if schema_version != SCHEMA_VERSION as u32 {
    bail!(
      concat!(
        "Cannot handle schema version {}. Expected {}. This might mean your dprint CLI ",
        "version is old and isn't able to get the latest information or the registry ",
        "needs to update its schema version.",
      ),
      schema_version,
      SCHEMA_VERSION
    );
  }

  let version = obj
    .take_string("version")
    .ok_or_else(|| anyhow!("Expected to find a version property in the data."))?;
  let url = obj.take_string("url").ok_or_else(|| anyhow!("Expected to find a url property in the data."))?;
  let checksum = obj.take_string("checksum");

  Ok(Some(PluginUpdateUrlInfo {
    version: version.to_string(),
    url: url.to_string(),
    checksum: checksum.map(|c| c.to_string()),
  }))
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::environment::TestEnvironmentBuilder;

  #[test]
  fn should_get_valid() {
    let mut builder = TestEnvironmentBuilder::new();
    builder
      .add_remote_file(
        "https://plugins.dprint.dev/plugin/latest.json",
        r#"{ "schemaVersion": 1, "url": "url", "version": "version" }"#,
      )
      .add_remote_file(
        "https://plugins.dprint.dev/plugin/latest-checksum.json",
        r#"{ "schemaVersion": 1, "url": "url2", "version": "version2", "checksum": "checksum" }"#,
      );
    let environment = builder.build();
    environment.clone().run_in_runtime(async move {
      assert_eq!(
        read_update_url(&environment, "https://plugins.dprint.dev/plugin/latest.json").await.unwrap(),
        Some(PluginUpdateUrlInfo {
          version: "version".to_string(),
          url: "url".to_string(),
          checksum: None,
        })
      );
      assert_eq!(
        read_update_url(&environment, "https://plugins.dprint.dev/plugin/latest-checksum.json")
          .await
          .unwrap(),
        Some(PluginUpdateUrlInfo {
          version: "version2".to_string(),
          url: "url2".to_string(),
          checksum: Some("checksum".to_string()),
        })
      );
    })
  }

  #[test]
  fn should_err_invalid() {
    let mut builder = TestEnvironmentBuilder::new();
    builder.add_remote_file(
      "https://plugins.dprint.dev/plugin/latest.json",
      r#"{ "schemaVersion": 205, "url": "url", "version": "version" }"#,
    );
    let environment = builder.build();
    environment.clone().run_in_runtime(async move {
      assert_eq!(
        read_update_url(&environment, "https://plugins.dprint.dev/plugin/latest.json")
          .await
          .err()
          .unwrap()
          .to_string(),
        concat!(
          "Cannot handle schema version 205. Expected 1. This might mean your dprint CLI version ",
          "is old and isn't able to get the latest information or the registry needs to update its schema version.",
        )
      );
      assert_eq!(
        read_update_url(&environment, "https://plugins.dprint.dev/plugin/not-exists.json")
          .await
          .unwrap(),
        None,
      );
    });
  }
}
