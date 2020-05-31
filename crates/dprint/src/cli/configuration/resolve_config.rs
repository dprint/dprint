use std::path::PathBuf;
use std::collections::HashMap;
use colored::Colorize;
use crate::cache::Cache;
use crate::configuration::{ConfigMap, ConfigMapValue, deserialize_config};
use crate::cli::CliArgs;
use crate::environment::Environment;
use crate::types::ErrBox;
use crate::utils::{ResolvedPath, ResolvedPathSource};

use super::resolve_config_path;

pub struct ResolvedConfig {
    pub resolved_path: ResolvedPath,
    pub base_path: PathBuf,
    pub config_map: ConfigMap,
}

pub async fn resolve_config_from_args<'a, TEnvironment : Environment>(
    args: &CliArgs,
    cache: &Cache<'a, TEnvironment>,
    environment: &TEnvironment,
) -> Result<ResolvedConfig, ErrBox> {
    let resolved_config_path = resolve_config_path(args, cache, environment).await?;
    let config_file_path = &resolved_config_path.resolved_path.file_path;
    let main_config_map = get_config_map_from_path(config_file_path, environment)?;

    let mut main_config_map = match main_config_map {
        Ok(main_config_map) => main_config_map,
        Err(err) => {
            // allow no config file when plugins are specified
            if !args.plugin_urls.is_empty() && !environment.path_exists(config_file_path) {
                let mut config_map = HashMap::new();
                // hack: easy way to supress project type diagnostic check
                config_map.insert(String::from("projectType"), ConfigMapValue::String(String::from("openSource")));
                config_map
            } else {
                return err!(
                    "No config file found at {}. Did you mean to create (dprint init) or specify one (--config <path>)?\n  Error: {}",
                    config_file_path.display(),
                    err.to_string(),
                )
            }
        }
    };

    if resolved_config_path.resolved_path.source != ResolvedPathSource::Local {
        // Careful! Ensure both of theses are removed.
        let removed_includes = main_config_map.remove("includes").is_some();
        let removed_excludes = main_config_map.remove("excludes").is_some();
        let was_removed = removed_includes || removed_excludes;
        if was_removed && resolved_config_path.resolved_path.is_first_download {
            environment.log(&get_warn_includes_excludes_message());
        }
    }

    Ok(ResolvedConfig {
        resolved_path: resolved_config_path.resolved_path,
        base_path: resolved_config_path.base_path,
        config_map: main_config_map,
    })
}

fn get_config_map_from_path(file_path: &PathBuf, environment: &impl Environment) -> Result<Result<ConfigMap, ErrBox>, ErrBox> {
    let config_file_text = match environment.read_file(file_path) {
        Ok(file_text) => file_text,
        Err(err) => return Ok(Err(err)),
    };

    let result = match deserialize_config(&config_file_text) {
        Ok(map) => map,
        Err(e) => return err!("Error deserializing {}. {}", file_path.display(), e.to_string()),
    };

    Ok(Ok(result))
}

fn get_warn_includes_excludes_message() -> String {
    format!(
        "{} The 'includes' and 'excludes' properties are ignored for security reasons on remote configuration.",
        "Note: ".bold().to_string()
    )
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use crate::cache::Cache;
    use crate::cli::parse_args;
    use crate::environment::{Environment, TestEnvironment};
    use crate::types::ErrBox;
    use crate::utils::ResolvedPathSource;

    use super::*;

    async fn get_result(url: &str, environment: &impl Environment) -> Result<ResolvedConfig, ErrBox> {
        let args = parse_args(vec![String::from(""), String::from("-c"), String::from(url)]).unwrap();
        let cache = Cache::new(environment).unwrap();
        resolve_config_from_args(&args, &cache, &environment).await
    }

    #[tokio::test]
    async fn it_should_get_local_config_file() {
        let environment = TestEnvironment::new();
        environment.write_file(&PathBuf::from("/test.json"), r#"{
            "projectType": "openSource",
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"],
            "includes": [],
            "excludes": []
        }"#).unwrap();

        let result = get_result("/test.json", &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0);
        assert_eq!(result.base_path, PathBuf::from("./"));
        assert_eq!(result.resolved_path.source, ResolvedPathSource::Local);
        assert_eq!(result.config_map.get("projectType").unwrap(), &ConfigMapValue::String(String::from("openSource")));
        assert_eq!(result.config_map.contains_key("includes"), true);
        assert_eq!(result.config_map.contains_key("excludes"), true);
    }

    #[tokio::test]
    async fn it_should_get_remote_config_file() {
        let environment = TestEnvironment::new();
        environment.add_remote_file("https://dprint.dev/test.json", r#"{
            "projectType": "openSource",
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#.as_bytes());

        let result = get_result("https://dprint.dev/test.json", &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0);
        assert_eq!(result.base_path, PathBuf::from("./"));
        assert_eq!(result.resolved_path.source, ResolvedPathSource::Remote);
        assert_eq!(result.config_map.get("projectType").unwrap(), &ConfigMapValue::String(String::from("openSource")));
    }

    #[tokio::test]
    async fn it_should_warn_on_first_download_for_remote_config_with_includes() {
        let environment = TestEnvironment::new();
        environment.add_remote_file("https://dprint.dev/test.json", r#"{
            "projectType": "openSource",
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"],
            "includes": []
        }"#.as_bytes());

        let result = get_result("https://dprint.dev/test.json", &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages(), vec![get_warn_includes_excludes_message()]);
        assert_eq!(result.config_map.contains_key("includes"), false);

        environment.clear_logs();
        let result = get_result("https://dprint.dev/test.json", &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0); // no warning this time
        assert_eq!(result.config_map.contains_key("includes"), false);
    }

    #[tokio::test]
    async fn it_should_warn_on_first_download_for_remote_config_with_excludes() {
        let environment = TestEnvironment::new();
        environment.add_remote_file("https://dprint.dev/test.json", r#"{
            "projectType": "openSource",
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"],
            "excludes": []
        }"#.as_bytes());

        let result = get_result("https://dprint.dev/test.json", &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages(), vec![get_warn_includes_excludes_message()]);
        assert_eq!(result.config_map.contains_key("excludes"), false);

        environment.clear_logs();
        let result = get_result("https://dprint.dev/test.json", &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0); // no warning this time
        assert_eq!(result.config_map.contains_key("excludes"), false);
    }

    #[tokio::test]
    async fn it_should_warn_on_first_download_for_remote_config_with_includes_and_excludes() {
        let environment = TestEnvironment::new();
        environment.add_remote_file("https://dprint.dev/test.json", r#"{
            "projectType": "openSource",
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"],
            "includes": [],
            "excludes": []
        }"#.as_bytes());

        let result = get_result("https://dprint.dev/test.json", &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages(), vec![get_warn_includes_excludes_message()]);
        assert_eq!(result.config_map.contains_key("includes"), false);
        assert_eq!(result.config_map.contains_key("excludes"), false);

        environment.clear_logs();
        let result = get_result("https://dprint.dev/test.json", &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0); // no warning this time
        assert_eq!(result.config_map.contains_key("includes"), false);
        assert_eq!(result.config_map.contains_key("excludes"), false);
    }

    #[tokio::test]
    async fn it_should_not_warn_remove_config_no_includes_or_excludes() {
        let environment = TestEnvironment::new();
        environment.add_remote_file("https://dprint.dev/test.json", r#"{
            "projectType": "openSource",
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#.as_bytes());

        get_result("https://dprint.dev/test.json", &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0);
    }
}