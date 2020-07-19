use crate::types::ErrBox;
use crate::utils::{PathSource, resolve_url_or_file_path_to_path_source};

#[derive(Clone, Debug, PartialEq)]
pub struct PluginSourceReference {
    pub path_source: PathSource,
    pub checksum: Option<String>,
}

impl PluginSourceReference {
    pub fn display(&self) -> String {
        self.path_source.display()
    }

    #[cfg(test)]
    pub fn new_local(path: std::path::PathBuf) -> PluginSourceReference {
        PluginSourceReference {
            path_source: PathSource::new_local(path),
            checksum: None,
        }
    }

    #[cfg(test)]
    pub fn new_remote_from_str(url: &str) -> PluginSourceReference {
        PluginSourceReference {
            path_source: PathSource::new_remote_from_str(url),
            checksum: None,
        }
    }
}

pub fn parse_plugin_source_reference(text: &str, base: &PathSource) -> Result<PluginSourceReference, ErrBox> {
    let parts = text.split("@").collect::<Vec<_>>();

    if parts.len() > 2 {
        return err!("Unexpected number of @ symbols in plugin text: {}", text);
    }

    Ok(PluginSourceReference {
        path_source: resolve_url_or_file_path_to_path_source(&parts[0], base)?,
        checksum: parts.get(1).map(|x| x.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use super::*;

    #[test]
    fn it_should_parse_plugin_without_checksum() {
        let result = parse_plugin_source_reference("http://dprint.dev/wasm_plugin.wasm", &PathSource::new_local(PathBuf::from("./"))).unwrap();
        assert_eq!(result, PluginSourceReference {
            path_source: PathSource::new_remote_from_str("http://dprint.dev/wasm_plugin.wasm"),
            checksum: None,
        });
    }

    #[test]
    fn it_should_parse_plugin_with_checksum() {
        let result = parse_plugin_source_reference("http://dprint.dev/wasm_plugin.wasm@checksum", &PathSource::new_local(PathBuf::from("./"))).unwrap();
        assert_eq!(result, PluginSourceReference {
            path_source: PathSource::new_remote_from_str("http://dprint.dev/wasm_plugin.wasm"),
            checksum: Some(String::from("checksum")),
        });
    }

    #[test]
    fn it_should_error_multiple_at_symbols() {
        let plugin_text = "http://dprint.dev/wasm_plugin.wasm@checksum@other";
        let err = parse_plugin_source_reference(&plugin_text, &PathSource::new_local(PathBuf::from("./"))).err().unwrap();
        assert_eq!(err.to_string(), format!("Unexpected number of @ symbols in plugin text: {}", plugin_text));
    }
}