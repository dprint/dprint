use std::path::PathBuf;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use dprint_core::configuration::GlobalConfiguration;
use super::{CliArgs, FormatContext, FormatContexts};
use crate::environment::Environment;
use crate::configuration::{self, ConfigMap, ConfigMapValue, get_global_config, get_plugin_config_map};
use crate::plugins::{initialize_plugin, Plugin, InitializedPlugin, PluginResolver};
use crate::utils::get_table_text;
use crate::types::ErrBox;

struct PluginWithConfig {
    pub plugin: Box<dyn Plugin>,
    pub config: HashMap<String, String>,
}

pub async fn run_cli(args: CliArgs, environment: &impl Environment, plugin_resolver: &impl PluginResolver) -> Result<(), ErrBox> {
    if args.help_text.is_some() {
        return output_help(&args, environment, plugin_resolver).await;
    }
    if args.version {
        return output_version(&args, environment, plugin_resolver).await;
    }

    if args.clear_cache {
        let cache_dir = environment.get_cache_dir()?; // this actually creates the directory, but whatever
        environment.remove_dir_all(&cache_dir)?;
        environment.log(&format!("Deleted {}", cache_dir.to_string_lossy()));
        return Ok(());
    }

    if args.init {
        init_config_file(environment).await?;
        environment.log("Created dprint.config.json");
        return Ok(());
    }

    let mut config_map = get_config_map_from_args(&args, environment)?;
    let file_paths = resolve_file_paths(&mut config_map, &args, environment)?;

    let plugins = resolve_plugins(&mut config_map, &args, plugin_resolver).await?;
    if plugins.is_empty() {
        return err!("No formatting plugins found. Ensure at least one is specified in the 'plugins' array of the configuration file.");
    }

    let project_type_result = check_project_type_diagnostic(&mut config_map);
    let global_config = get_global_config(config_map, environment)?;

    if args.output_resolved_config {
        return output_resolved_config(plugins, &global_config, environment);
    }

    let format_contexts = get_plugin_format_contexts(plugins, file_paths);

    if args.output_file_paths {
        output_file_paths(format_contexts.iter().flat_map(|x| x.file_paths.iter()), environment);
        return Ok(());
    }

    if format_contexts.is_empty() {
        return err!("No files found to format with the specified plugins. You may want to try using `--output-file-paths` to see which files it's finding.");
    }

    // surface the project type error at this point
    project_type_result?;

    if args.check {
        check_files(format_contexts, global_config, environment).await
    } else {
        format_files(format_contexts, global_config, environment).await
    }
}

fn get_plugin_format_contexts(plugins_with_config: Vec<PluginWithConfig>, file_paths: Vec<PathBuf>) -> Vec<FormatContext> {
    let mut file_paths_by_plugin: HashMap<String, Vec<PathBuf>> = HashMap::new();

    for file_path in file_paths.into_iter() {
        if let Some(file_extension) = crate::utils::get_lowercase_file_extension(&file_path) {
            if let Some(plugin_with_config) = plugins_with_config.iter().filter(|p| p.plugin.file_extensions().contains(&file_extension)).next() {
                if let Some(file_paths) = file_paths_by_plugin.get_mut(plugin_with_config.plugin.name()) {
                    file_paths.push(file_path);
                } else {
                    file_paths_by_plugin.insert(String::from(plugin_with_config.plugin.name()), vec![file_path]);
                }
                continue;
            }
        }
    }

    let mut format_contexts = Vec::new();
    for plugin_with_config in plugins_with_config.into_iter() {
        if let Some(file_paths) = file_paths_by_plugin.remove(plugin_with_config.plugin.name()) {
            format_contexts.push(FormatContext {
                plugin: plugin_with_config.plugin,
                config: plugin_with_config.config,
                file_paths,
            });
        }
    }

    format_contexts
}

async fn output_version(args: &CliArgs, environment: &impl Environment, plugin_resolver: &impl PluginResolver) -> Result<(), ErrBox> {
    // log the cli's current version first
    environment.log(&format!("{} v{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")));

    // now check for the plugins
    for plugin in get_plugins_from_args(args, environment, plugin_resolver).await? {
        // output their names and versions
        environment.log(&format!("{} v{}", plugin.name(), plugin.version()));
    }

    Ok(())
}

async fn output_help(args: &CliArgs, environment: &impl Environment, plugin_resolver: &impl PluginResolver) -> Result<(), ErrBox> {
    // log the cli's help first
    environment.log(args.help_text.as_ref().unwrap());

    // now check for the plugins
    let plugins = get_plugins_from_args(args, environment, plugin_resolver).await?;
    if !plugins.is_empty() {
        let plugin_texts = get_table_text(plugins.iter().map(|plugin| (plugin.name(), plugin.help_url())).collect(), 4);
        environment.log("\nPLUGINS HELP:");
        for plugin_text in plugin_texts {
            // output their names and help urls
            environment.log(&format!("    {}", plugin_text));
        }
    }

    Ok(())
}

async fn get_plugins_from_args(args: &CliArgs, environment: &impl Environment, plugin_resolver: &impl PluginResolver) -> Result<Vec<Box<dyn Plugin>>, ErrBox> {
    match get_config_map_from_args(args, environment) {
        Ok(config_map) => {
            let mut config_map = config_map;
            let plugins_with_config = resolve_plugins(&mut config_map, args, plugin_resolver).await?;
            Ok(plugins_with_config.into_iter().map(|p| p.plugin).collect())
        },
        Err(_) => {
            // ignore
            Ok(Vec::new())
        }
    }
}

fn output_file_paths<'a>(file_paths: impl Iterator<Item=&'a PathBuf>, environment: &impl Environment) {
    for file_path in file_paths {
        environment.log(&file_path.to_string_lossy())
    }
}

fn output_resolved_config(
    plugins_with_config: Vec<PluginWithConfig>,
    global_config: &GlobalConfiguration,
    environment: &impl Environment,
) -> Result<(), ErrBox> {
    for plugin_with_config in plugins_with_config {
        let config_keys = plugin_with_config.plugin.config_keys().to_owned();
        let initialized_plugin = initialize_plugin(
            plugin_with_config.plugin,
            plugin_with_config.config,
            global_config,
            environment,
        )?;
        let text = initialized_plugin.get_resolved_config();

        let key_values: HashMap<String, String> = serde_json::from_str(&text).unwrap();
        let pretty_text = serde_json::to_string_pretty(&key_values).unwrap();
        environment.log(&format!("{}: {}", config_keys.join("/"), pretty_text));
    }

    Ok(())
}

async fn init_config_file(environment: &impl Environment) -> Result<(), ErrBox> {
    let config_file_path = PathBuf::from("./dprint.config.json");
    if !environment.path_exists(&config_file_path) {
        environment.write_file(&config_file_path, &configuration::get_init_config_file_text(environment).await?)
    } else {
        err!("Configuration file 'dprint.config.json' already exists in current working directory.")
    }
}

async fn check_files(format_contexts: FormatContexts, global_config: GlobalConfiguration, environment: &impl Environment) -> Result<(), ErrBox> {
    let not_formatted_files_count = Arc::new(AtomicUsize::new(0));

    let result = run_parallelized(format_contexts, global_config, environment, {
        let not_formatted_files_count = not_formatted_files_count.clone();
        move |plugin, file_path, file_text, _| {
            let formatted_text = plugin.format_text(&file_path, &file_text)?;
            if formatted_text != file_text {
                not_formatted_files_count.fetch_add(1, Ordering::SeqCst);
            }
            Ok(())
        }
    }).await;

    if let Err(err) = result {
        return err!(
            "A panic occurred in a Dprint plugin. You may want to run in verbose mode (--verbose) to help figure out where it failed then report this as a.\n  Error: {}",
            err.to_string()
        );
    }

    let not_formatted_files_count = not_formatted_files_count.load(Ordering::SeqCst);
    if not_formatted_files_count == 0 {
        Ok(())
    } else {
        let f = if not_formatted_files_count == 1 { "file" } else { "files" };
        err!("Found {} not formatted {}.", not_formatted_files_count, f)
    }
}

async fn format_files(format_contexts: FormatContexts, global_config: GlobalConfiguration, environment: &impl Environment) -> Result<(), ErrBox> {
    let formatted_files_count = Arc::new(AtomicUsize::new(0));
    let files_count: usize = format_contexts.iter().map(|x| x.file_paths.len()).sum();

    run_parallelized(format_contexts, global_config, environment, {
        let formatted_files_count = formatted_files_count.clone();
        move |plugin, file_path, file_text, environment| {
            let formatted_text = plugin.format_text(&file_path, &file_text)?;
            if formatted_text != file_text {
                environment.write_file(&file_path, &formatted_text)?;
                formatted_files_count.fetch_add(1, Ordering::SeqCst);
            }
            Ok(())
        }
    }).await?;

    let formatted_files_count = formatted_files_count.load(Ordering::SeqCst);
    if formatted_files_count > 0 {
        let suffix = if files_count == 1 { "file" } else { "files" };
        environment.log(&format!("Formatted {} {}.", formatted_files_count, suffix));
    }

    Ok(())
}

async fn run_parallelized<F, TEnvironment : Environment>(
    format_contexts: FormatContexts,
    global_config: GlobalConfiguration,
    environment: &TEnvironment,
    f: F,
) -> Result<(), ErrBox> where F: Fn(&Box<dyn InitializedPlugin>, &PathBuf, String, &TEnvironment) -> Result<(), ErrBox> + Send + 'static + Clone {
    // At the moment this is parallelized across plugins because Wasmer instances can't be shared or sent between threads.
    let error_count = Arc::new(AtomicUsize::new(0));
    let handles = format_contexts.into_iter().map(|format_context| {
        let environment = environment.to_owned();
        let global_config = global_config.to_owned();
        let f = f.clone();
        let error_count = error_count.clone();
        tokio::task::spawn_blocking(move || {
            let plugin_name = format_context.plugin.name().to_string();
            let result = inner_run(format_context, global_config, &environment, f);
            if let Err(err) = result {
                environment.log_error(&format!("[{}]: {}", plugin_name, err.to_string()));
                error_count.fetch_add(1, Ordering::SeqCst);
            }
        })
    });

    futures::future::try_join_all(handles).await?;

    let error_count = error_count.load(Ordering::SeqCst);
    return if error_count == 0 {
        Ok(())
    } else {
        err!("Had {0} error(s) formatting.", error_count)
    };

    #[inline]
    fn inner_run<F, TEnvironment : Environment>(
        format_context: FormatContext,
        global_config: GlobalConfiguration,
        environment: &TEnvironment,
        f: F
    ) -> Result<(), ErrBox> where F: Fn(&Box<dyn InitializedPlugin>, &PathBuf, String, &TEnvironment) -> Result<(), ErrBox> + Send + 'static + Clone {
        let initialized_plugin = initialize_plugin(
            format_context.plugin,
            format_context.config,
            &global_config,
            environment,
        )?;

        for file_path in format_context.file_paths {
            match run_for_file_path(&file_path, environment, &initialized_plugin, &f) {
                Ok(_) => {},
                Err(err) => return err!("Error formatting {}. Message: {}", file_path.to_string_lossy(), err.to_string()),
            }
        }

        return Ok(());
    }

    #[inline]
    fn run_for_file_path<F, TEnvironment : Environment>(
        file_path: &PathBuf,
        environment: &TEnvironment,
        initialized_plugin: &Box<dyn InitializedPlugin>,
        f: &F
    ) -> Result<(), ErrBox> where F: Fn(&Box<dyn InitializedPlugin>, &PathBuf, String, &TEnvironment) -> Result<(), ErrBox> + Send + 'static + Clone {
        let file_text = environment.read_file(&file_path)?;
        f(initialized_plugin, &file_path, file_text, &environment)
    }
}

async fn resolve_plugins(config_map: &mut ConfigMap, args: &CliArgs, plugin_resolver: &impl PluginResolver) -> Result<Vec<PluginWithConfig>, ErrBox> {
    let plugin_urls = get_plugin_urls(config_map, args)?;
    let plugins = plugin_resolver.resolve_plugins(&plugin_urls).await?;
    let mut plugins_with_config = Vec::new();

    for plugin in plugins.into_iter() {
        plugins_with_config.push(PluginWithConfig {
            config: get_plugin_config_map(&plugin, config_map)?,
            plugin,
        });
    }

    return Ok(plugins_with_config);

    fn get_plugin_urls(config_map: &mut ConfigMap, args: &CliArgs) -> Result<Vec<String>, ErrBox> {
        let plugin_urls_from_config = take_array_from_config_map(config_map, "plugins")?;

        Ok(if args.plugin_urls.is_empty() {
            plugin_urls_from_config
        } else {
            args.plugin_urls.clone()
        })
    }
}

fn check_project_type_diagnostic(config_map: &mut ConfigMap) -> Result<(), ErrBox> {
    if let Some(diagnostic) = configuration::handle_project_type_diagnostic(config_map) {
        return err!("{}", diagnostic.message);
    }

    Ok(())
}

fn resolve_file_paths(config_map: &mut ConfigMap, args: &CliArgs, environment: &impl Environment) -> Result<Vec<PathBuf>, ErrBox> {
    let mut file_patterns = Vec::new();
    let includes = take_array_from_config_map(config_map, "includes")?;
    let excludes = take_array_from_config_map(config_map, "excludes")?;

    file_patterns.extend(if args.file_patterns.is_empty() {
        includes
    } else {
        args.file_patterns.clone()
    });

    file_patterns.extend(if args.exclude_file_patterns.is_empty() {
        excludes
    } else {
        args.exclude_file_patterns.clone()
    }.into_iter().map(|exclude| if exclude.starts_with("!") { exclude } else { format!("!{}", exclude) }));

    if !args.allow_node_modules {
        // glob walker will not search the children of a directory once it's ignored like this
        file_patterns.push(String::from("!**/node_modules"));
    }

    // glob walker doesn't support having `./` at the front of paths, so just remove them when they appear
    for file_pattern in file_patterns.iter_mut() {
        if file_pattern.starts_with("./") {
            *file_pattern = String::from(&file_pattern[2..]);
        }
        if file_pattern.starts_with("!./") {
            *file_pattern = format!("!{}", &file_pattern[3..]);
        }
    }

    environment.glob(&file_patterns)
}

fn get_config_map_from_args(args: &CliArgs, environment: &impl Environment) -> Result<ConfigMap, ErrBox> {
    let config_path = PathBuf::from(args.config.as_ref().map(|x| x.to_owned()).unwrap_or(String::from("./dprint.config.json")));
    let config_file_text = match environment.read_file(&config_path) {
        Ok(file_text) => file_text,
        Err(err) => {
            // allow no config file when plugins are specified
            if !args.plugin_urls.is_empty() && !environment.path_exists(&config_path) {
                let mut config_map = HashMap::new();
                // hack: easy way to supress project type diagnostic check
                config_map.insert(String::from("projectType"), ConfigMapValue::String(String::from("openSource")));
                return Ok(config_map);
            }

            return err!(
                "No config file found at {}. Did you mean to create (dprint --init) or specify one (dprint --config <path>)?\n  Error: {}",
                config_path.to_string_lossy(),
                err.to_string(),
            )
        },
    };

    let result = match configuration::deserialize_config(&config_file_text) {
        Ok(map) => map,
        Err(e) => return err!("Error deserializing. {}", e.to_string()),
    };

    Ok(result)
}

// todo: move somewhere else (maybe make a wrapper around ConfigMap)
fn take_array_from_config_map(config_map: &mut ConfigMap, property_name: &str) -> Result<Vec<String>, ErrBox> {
    let mut result = Vec::new();
    if let Some(value) = config_map.remove(property_name) {
        match value {
            ConfigMapValue::Vec(elements) => {
                result.extend(elements);
            },
            _ => return err!("Expected array in '{}' property.", property_name),
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;
    use crate::environment::{Environment, TestEnvironment};
    use crate::configuration::*;
    use crate::plugins::wasm::WasmPluginResolver;
    use crate::plugins::CompilationResult;
    use crate::types::ErrBox;

    use super::run_cli;
    use super::super::parse_args;

    async fn run_test_cli(args: Vec<&'static str>, environment: &impl Environment) -> Result<(), ErrBox> {
        let mut args: Vec<String> = args.into_iter().map(String::from).collect();
        args.insert(0, String::from(""));
        let plugin_resolver = WasmPluginResolver::new(environment, &quick_compile);
        let args = parse_args(args)?;
        run_cli(args, environment, &plugin_resolver).await
    }

    #[tokio::test]
    async fn it_should_output_version_with_no_plugins() {
        let environment = TestEnvironment::new();
        run_test_cli(vec!["--version"], &environment).await.unwrap();
        let logged_messages = environment.get_logged_messages();
        assert_eq!(logged_messages, vec![format!("dprint v{}", env!("CARGO_PKG_VERSION"))]);
    }

    #[tokio::test]
    async fn it_should_output_version_with_plugins() {
        let environment = get_test_environment_with_remote_plugin();
        environment.write_file(&PathBuf::from("./dprint.config.json"), r#"{
            "projectType": "openSource",
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#).unwrap();

        run_test_cli(vec!["--version"], &environment).await.unwrap();
        let logged_messages = environment.get_logged_messages();
        assert_eq!(logged_messages, vec![
            format!("dprint v{}", env!("CARGO_PKG_VERSION")),
            String::from("Compiling wasm module..."), // this should happen after getting dprint version
            String::from("test-plugin v0.1.0")
        ]);

        environment.clear_logs();
        run_test_cli(vec!["--version"], &environment).await.unwrap();
        let logged_messages = environment.get_logged_messages();
        assert_eq!(logged_messages, vec![
            format!("dprint v{}", env!("CARGO_PKG_VERSION")),
            String::from("test-plugin v0.1.0")
        ]);
    }

    #[tokio::test]
    async fn it_should_output_help_with_no_plugins() {
        let environment = TestEnvironment::new();
        run_test_cli(vec!["--help"], &environment).await.unwrap();
        let logged_messages = environment.get_logged_messages();
        assert_eq!(logged_messages, vec![get_expected_help_text()]);
    }

    #[tokio::test]
    async fn it_should_output_help_text_with_plugins() {
        let environment = get_test_environment_with_remote_plugin();
        environment.write_file(&PathBuf::from("./dprint.config.json"), r#"{
            "projectType": "openSource",
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm", "https://plugins.dprint.dev/test-plugin.wasm"]
        }"#).unwrap();

        // run it once to initialize the plugins (this is not a big deal)
        run_test_cli(vec!["--help"], &environment).await.unwrap();
        environment.clear_logs();

        run_test_cli(vec!["--help"], &environment).await.unwrap();
        let logged_messages = environment.get_logged_messages();
        assert_eq!(logged_messages, vec![
            get_expected_help_text(),
            "\nPLUGINS HELP:",
            "    test-plugin https://dprint.dev/plugins/test",
            "    test-plugin https://dprint.dev/plugins/test"
        ]);
    }

    #[tokio::test]
    async fn it_should_output_resolve_config() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        run_test_cli(vec!["--output-resolved-config"], &environment).await.unwrap();
        let logged_messages = environment.get_logged_messages();
        assert_eq!(logged_messages, vec!["test-plugin: {\n  \"ending\": \"formatted\"\n}"]);
    }

    #[tokio::test]
    async fn it_should_output_resolved_file_paths() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        environment.write_file(&PathBuf::from("/file.txt"), "const t=4;").unwrap();
        environment.write_file(&PathBuf::from("/file2.txt"), "const t=4;").unwrap();
        run_test_cli(vec!["--output-file-paths", "**/*.txt"], &environment).await.unwrap();
        let mut logged_messages = environment.get_logged_messages();
        logged_messages.sort();
        assert_eq!(logged_messages, vec!["/file.txt", "/file2.txt"]);
    }

    #[tokio::test]
    async fn it_should_not_output_file_paths_not_supported_by_plugins() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        environment.write_file(&PathBuf::from("/file.ts"), "const t=4;").unwrap();
        environment.write_file(&PathBuf::from("/file2.ts"), "const t=4;").unwrap();
        run_test_cli(vec!["--output-file-paths", "**/*.ts"], &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0);
    }

    #[tokio::test]
    async fn it_should_format_files() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        let file_path = PathBuf::from("/file.txt");
        environment.write_file(&file_path, "text").unwrap();
        run_test_cli(vec!["/file.txt"], &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages(), vec!["Formatted 1 file."]);
        assert_eq!(environment.get_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path).unwrap(), "text_formatted");
    }

    #[tokio::test]
    async fn it_should_format_when_specifying_dot_slash_paths() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        let file_path = PathBuf::from("/file.txt");
        environment.write_file(&file_path, "text").unwrap();
        run_test_cli(vec!["./file.txt"], &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages(), vec!["Formatted 1 file."]);
        assert_eq!(environment.get_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path).unwrap(), "text_formatted");
    }

    #[tokio::test]
    async fn it_should_exclude_a_specified_dot_slash_path() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        let file_path = PathBuf::from("/file.txt");
        environment.write_file(&file_path, "text").unwrap();
        let file_path2 = PathBuf::from("/file2.txt");
        environment.write_file(&file_path2, "text").unwrap();
        run_test_cli(vec!["./**/*.txt", "--excludes", "./file2.txt"], &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages(), vec!["Formatted 1 file."]);
        assert_eq!(environment.get_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path).unwrap(), "text_formatted");
        assert_eq!(environment.read_file(&file_path2).unwrap(), "text");
    }

    #[tokio::test]
    async fn it_should_ignore_files_in_node_modules_by_default() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        environment.write_file(&PathBuf::from("/node_modules/file.txt"), "").unwrap();
        environment.write_file(&PathBuf::from("/test/node_modules/file.txt"), "").unwrap();
        environment.write_file(&PathBuf::from("/file.txt"), "").unwrap();
        run_test_cli(vec!["**/*.txt"], &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages(), vec!["Formatted 1 file."]);
        assert_eq!(environment.get_logged_errors().len(), 0);
    }

    #[tokio::test]
    async fn it_should_not_ignore_files_in_node_modules_when_allowed() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        environment.write_file(&PathBuf::from("/node_modules/file.txt"), "const t=4;").unwrap();
        environment.write_file(&PathBuf::from("/test/node_modules/file.txt"), "const t=4;").unwrap();
        run_test_cli(vec!["--allow-node-modules", "**/*.txt"], &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages(), vec!["Formatted 2 files."]);
        assert_eq!(environment.get_logged_errors().len(), 0);
    }

    #[tokio::test]
    async fn it_should_format_files_with_config() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        let file_path1 = PathBuf::from("/file1.txt");
        let file_path2 = PathBuf::from("/file2.txt");
        environment.write_file(&PathBuf::from("/config.json"), r#"{
            "projectType": "openSource",
            "test-plugin": {
                "ending": "custom-formatted"
            },
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#).unwrap();
        environment.write_file(&file_path1, "text").unwrap();
        environment.write_file(&file_path2, "text2").unwrap();

        run_test_cli(vec!["--config", "/config.json", "/file1.txt", "/file2.txt"], &environment).await.unwrap();

        assert_eq!(environment.get_logged_messages(), vec!["Formatted 2 files."]);
        assert_eq!(environment.get_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "text_custom-formatted");
        assert_eq!(environment.read_file(&file_path2).unwrap(), "text2_custom-formatted");
    }

    #[tokio::test]
    async fn it_should_format_files_with_config_using_c() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        let file_path1 = PathBuf::from("/file1.txt");
        environment.write_file(&file_path1, "text").unwrap();
        environment.write_file(&PathBuf::from("/config.json"), r#"{
            "projectType": "openSource",
            "test-plugin": { "ending": "custom-formatted" },
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#).unwrap();

        run_test_cli(vec!["-c", "/config.json", "/file1.txt"], &environment).await.unwrap();

        assert_eq!(environment.get_logged_messages(), vec!["Formatted 1 file."]);
        assert_eq!(environment.get_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "text_custom-formatted");
    }

    #[tokio::test]
    async fn it_should_error_when_config_file_does_not_exist() {
        let environment = TestEnvironment::new();
        environment.write_file(&PathBuf::from("/test.txt"), "test").unwrap();

        let error_message = run_test_cli(vec!["**/*.txt"], &environment).await.err().unwrap();

        assert_eq!(
            error_message.to_string(),
            concat!(
                "No config file found at ./dprint.config.json. Did you mean to create (dprint --init) or specify one (dprint --config <path>)?\n",
                "  Error: Could not find file at path ./dprint.config.json"
            )
        );
        assert_eq!(environment.get_logged_messages().len(), 0);
        assert_eq!(environment.get_logged_errors().len(), 0);
    }

    #[tokio::test]
    async fn it_should_error_on_plugin_config_diagnostic() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        environment.write_file(&PathBuf::from("./dprint.config.json"), r#"{
            "projectType": "openSource",
            "test-plugin": { "non-existent": 25 },
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#).unwrap();
        environment.write_file(&PathBuf::from("/test.txt"), "test").unwrap();

        let error_message = run_test_cli(vec!["**/*.txt"], &environment).await.err().unwrap();

        assert_eq!(error_message.to_string(), "Had 1 error(s) formatting.");
        assert_eq!(environment.get_logged_messages().len(), 0);
        assert_eq!(environment.get_logged_errors(), vec![
            "[test-plugin]: Unknown property in configuration: non-existent",
            "[test-plugin]: Error initializing from configuration file. Had 1 diagnostic(s)."
        ]);
    }

    #[tokio::test]
    async fn it_should_error_when_no_plugins_specified() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        environment.write_file(&PathBuf::from("./dprint.config.json"), r#"{
            "projectType": "openSource",
            "plugins": []
        }"#).unwrap();
        environment.write_file(&PathBuf::from("/test.txt"), "test").unwrap();

        let error_message = run_test_cli(vec!["**/*.txt"], &environment).await.err().unwrap();

        assert_eq!(error_message.to_string(), "No formatting plugins found. Ensure at least one is specified in the 'plugins' array of the configuration file.");
        assert_eq!(environment.get_logged_messages().len(), 0);
        assert_eq!(environment.get_logged_errors().len(), 0);
    }

    #[tokio::test]
    async fn it_should_use_plugins_specified_in_cli_args() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        environment.write_file(&PathBuf::from("./dprint.config.json"), r#"{
            "projectType": "openSource",
            "plugins": ["https://plugins.dprint.dev/test"]
        }"#).unwrap();
        environment.write_file(&PathBuf::from("/test.txt"), "test").unwrap();

        run_test_cli(vec!["**/*.txt", "--plugins", "https://plugins.dprint.dev/test-plugin.wasm"], &environment).await.unwrap();

        assert_eq!(environment.get_logged_messages(), vec!["Formatted 1 file."]);
        assert_eq!(environment.get_logged_errors().len(), 0);
    }

    #[tokio::test]
    async fn it_should_allow_using_no_config_when_plugins_specified() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        environment.remove_file(&PathBuf::from("./dprint.config.json")).unwrap();
        environment.write_file(&PathBuf::from("/test.txt"), "test").unwrap();

        run_test_cli(vec!["**/*.txt", "--plugins", "https://plugins.dprint.dev/test-plugin.wasm"], &environment).await.unwrap();

        assert_eq!(environment.get_logged_messages(), vec!["Formatted 1 file."]);
        assert_eq!(environment.get_logged_errors().len(), 0);
    }

    #[tokio::test]
    async fn it_should_error_when_no_files_match_glob() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        let error_message = run_test_cli(vec!["**/*.txt"], &environment).await.err().unwrap();

        assert_eq!(
            error_message.to_string(),
            concat!(
                "No files found to format with the specified plugins. ",
                "You may want to try using `--output-file-paths` to see which files it's finding."
            )
        );
        assert_eq!(environment.get_logged_messages().len(), 0);
        assert_eq!(environment.get_logged_errors().len(), 0);
    }

    #[tokio::test]
    async fn it_should_format_files_with_config_includes() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        let file_path1 = PathBuf::from("/file1.txt");
        let file_path2 = PathBuf::from("/file2.txt");
        environment.write_file(&file_path1, "text1").unwrap();
        environment.write_file(&file_path2, "text2").unwrap();
        environment.write_file(&PathBuf::from("./dprint.config.json"), r#"{
            "projectType": "openSource",
            "includes": ["**/*.txt"],
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#).unwrap();

        run_test_cli(vec![], &environment).await.unwrap();

        assert_eq!(environment.get_logged_messages(), vec!["Formatted 2 files."]);
        assert_eq!(environment.get_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "text1_formatted");
        assert_eq!(environment.read_file(&file_path2).unwrap(), "text2_formatted");
    }

    #[tokio::test]
    async fn it_should_override_config_includes_with_cli_includes() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        let file_path1 = PathBuf::from("/file1.txt");
        let file_path2 = PathBuf::from("/file2.txt");
        environment.write_file(&file_path1, "text1").unwrap();
        environment.write_file(&file_path2, "text2").unwrap();
        environment.write_file(&PathBuf::from("./dprint.config.json"), r#"{
            "projectType": "openSource",
            "includes": ["**/*.txt"],
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#).unwrap();

        run_test_cli(vec!["/file1.txt"], &environment).await.unwrap();

        assert_eq!(environment.get_logged_messages(), vec!["Formatted 1 file."]);
        assert_eq!(environment.get_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "text1_formatted");
        assert_eq!(environment.read_file(&file_path2).unwrap(), "text2");
    }

    #[tokio::test]
    async fn it_should_override_config_excludes_with_cli_excludes() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        let file_path1 = PathBuf::from("/file1.txt");
        let file_path2 = PathBuf::from("/file2.txt");
        environment.write_file(&file_path1, "text1").unwrap();
        environment.write_file(&file_path2, "text2").unwrap();
        environment.write_file(&PathBuf::from("./dprint.config.json"), r#"{
            "projectType": "openSource",
            "includes": ["**/*.txt"],
            "excludes": ["/file1.txt"],
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#).unwrap();

        run_test_cli(vec!["--excludes", "/file2.txt"], &environment).await.unwrap();

        assert_eq!(environment.get_logged_messages(), vec!["Formatted 1 file."]);
        assert_eq!(environment.get_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "text1_formatted");
        assert_eq!(environment.read_file(&file_path2).unwrap(), "text2");
    }


    #[tokio::test]
    async fn it_should_override_config_includes_and_excludes_with_cli() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        let file_path1 = PathBuf::from("/file1.txt");
        let file_path2 = PathBuf::from("/file2.txt");
        environment.write_file(&file_path1, "text1").unwrap();
        environment.write_file(&file_path2, "text2").unwrap();
        environment.write_file(&PathBuf::from("./dprint.config.json"), r#"{
            "projectType": "openSource",
            "includes": ["/file2.txt"],
            "excludes": ["/file1.txt"],
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#).unwrap();

        run_test_cli(vec!["/file1.txt", "--excludes", "/file2.txt"], &environment).await.unwrap();

        assert_eq!(environment.get_logged_messages(), vec!["Formatted 1 file."]);
        assert_eq!(environment.get_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "text1_formatted");
        assert_eq!(environment.read_file(&file_path2).unwrap(), "text2");
    }

    #[tokio::test]
    async fn it_should_format_files_with_config_excludes() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        let file_path1 = PathBuf::from("/file1.txt");
        let file_path2 = PathBuf::from("/file2.txt");
        environment.write_file(&file_path1, "text1").unwrap();
        environment.write_file(&file_path2, "text2").unwrap();
        environment.write_file(&PathBuf::from("./dprint.config.json"), r#"{
            "projectType": "openSource",
            "includes": ["**/*.txt"],
            "excludes": ["/file2.txt"],
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#).unwrap();

        run_test_cli(vec![], &environment).await.unwrap();

        assert_eq!(environment.get_logged_messages(), vec!["Formatted 1 file."]);
        assert_eq!(environment.get_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "text1_formatted");
        assert_eq!(environment.read_file(&file_path2).unwrap(), "text2");
    }

    #[tokio::test]
    async fn it_should_error_when_missing_project_type() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        environment.write_file(&PathBuf::from("./dprint.config.json"), r#"{
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#).unwrap();
        environment.write_file(&PathBuf::from("/file1.txt"), "text1_formatted").unwrap();
        let error_message = run_test_cli(vec!["/file1.txt"], &environment).await.err().unwrap();
        assert_eq!(error_message.to_string().find("The 'projectType' property").is_some(), true);
        assert_eq!(environment.get_logged_messages().len(), 0);
        assert_eq!(environment.get_logged_errors().len(), 0);
    }

    #[tokio::test]
    async fn it_should_not_output_when_no_files_need_formatting() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        environment.write_file(&PathBuf::from("/file.txt"), "text_formatted").unwrap();
        run_test_cli(vec!["/file.txt"], &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0);
        assert_eq!(environment.get_logged_errors().len(), 0);
    }

    #[tokio::test]
    async fn it_should_not_output_when_no_files_need_formatting_for_check() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        let file_path = PathBuf::from("/file.txt");
        environment.write_file(&file_path, "text_formatted").unwrap();
        run_test_cli(vec!["--check", "/file.txt"], &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0);
        assert_eq!(environment.get_logged_errors().len(), 0);
    }

    #[tokio::test]
    async fn it_should_output_when_a_file_need_formatting_for_check() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        environment.write_file(&PathBuf::from("/file.txt"), "const t=4;").unwrap();
        let error_message = run_test_cli(vec!["--check", "/file.txt"], &environment).await.err().unwrap();
        assert_eq!(error_message.to_string(), "Found 1 not formatted file.");
        assert_eq!(environment.get_logged_messages().len(), 0);
        assert_eq!(environment.get_logged_errors().len(), 0);
    }

    #[tokio::test]
    async fn it_should_output_when_files_need_formatting_for_check() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        environment.write_file(&PathBuf::from("/file1.txt"), "const t=4;").unwrap();
        environment.write_file(&PathBuf::from("/file2.txt"), "const t=4;").unwrap();

        let error_message = run_test_cli(vec!["--check", "/file1.txt", "/file2.txt"], &environment).await.err().unwrap();
        assert_eq!(error_message.to_string(), "Found 2 not formatted files.");
        assert_eq!(environment.get_logged_messages().len(), 0);
        assert_eq!(environment.get_logged_errors().len(), 0);
    }

    #[tokio::test]
    async fn it_should_initialize() {
        let environment = TestEnvironment::new();
        environment.add_remote_file(crate::plugins::REMOTE_INFO_URL, r#"{
            "schemaVersion": 1,
            "pluginSystemSchemaVersion": 1,
            "latest": [{
                "name": "dprint-plugin-typescript",
                "version": "0.17.2",
                "url": "https://plugins.dprint.dev/typescript-0.17.2.wasm",
                "configKey": "typescript"
            }, {
                "name": "dprint-plugin-jsonc",
                "version": "0.2.3",
                "url": "https://plugins.dprint.dev/json-0.2.3.wasm",
                "configKey": "json"
            }]
        }"#.as_bytes());
        let expected_text = get_init_config_file_text(&environment).await.unwrap();
        environment.clear_logs();
        run_test_cli(vec!["--init"], &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages(), vec![
            "What kind of project will Dprint be formatting?\n\nSponsor at: https://dprint.dev/sponsor\n",
            "Created dprint.config.json"
        ]);
        assert_eq!(environment.read_file(&PathBuf::from("./dprint.config.json")).unwrap(), expected_text);
    }

    #[tokio::test]
    async fn it_should_error_when_config_file_exists_on_initialize() {
        let environment = TestEnvironment::new();
        environment.write_file(&PathBuf::from("./dprint.config.json"), "{}").unwrap();
        let error_message = run_test_cli(vec!["--init"], &environment).await.err().unwrap();
        assert_eq!(error_message.to_string(), "Configuration file 'dprint.config.json' already exists in current working directory.");
    }

    #[tokio::test]
    async fn it_should_clear_cache_directory() {
        let environment = TestEnvironment::new();
        run_test_cli(vec!["--clear-cache"], &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages(), vec!["Deleted /cache"]);
        assert_eq!(environment.is_dir_deleted(&PathBuf::from("/cache")), true);
    }

    fn get_expected_help_text() -> &'static str {
        concat!("dprint ", env!("CARGO_PKG_VERSION"), r#"
Copyright 2020 by David Sherret

Auto-formats source code based on the specified plugins.

USAGE:
    dprint [OPTIONS] [--] [file patterns]...

OPTIONS:
        --allow-node-modules        Allows traversing node module directories (unstable - This flag will be renamed to
                                    be non-node specific in the future).
        --check                     Checks for any files that haven't been formatted.
        --clear-cache               Deletes the plugin cache directory.
    -c, --config <config>           Path to JSON configuration file. Defaults to ./dprint.config.json when not provided.
        --excludes <patterns>...    List of patterns to exclude files or directories when formatting (globs in quotes
                                    separated by spaces). This overrides what is specified in the config file.
        --init                      Initializes a configuration file in the current directory.
        --output-file-paths         Prints the resolved file paths.
        --output-resolved-config    Prints the resolved configuration.
        --plugins <urls>...         List of urls of plugins to use (urls separated by spaces). This overrides what is
                                    specified in the config file.
        --verbose                   Prints additional diagnostic information.
    -v, --version                   Prints the version.

ARGS:
    <file patterns>...    List of patterns used to find files to format (globs in quotes separated by spaces). This
                          overrides what is specified in the config file.

EXAMPLES:
    Create a dprint.config.json file:

      dprint --init

    Write formatted files to file system using the config file at ./dprint.config.json:

      dprint

    Check for any files that haven't been formatted:

      dprint --check

    Specify path to config file other than the default:

      dprint --config configs/dprint.config.json

    Write using the specified config and file paths:

      dprint --config formatting.config.json "**/*.{ts,tsx,js,jsx,json}""#)
    }

    // If this file doesn't exist, run `./build.ps1` in test/plugin. (Please consider helping me do something better here :))
    static PLUGIN_BYTES: &'static [u8] = include_bytes!("../../test/test_plugin.wasm");
    lazy_static! {
        // cache the compilation so this only has to be done once across all tests
        static ref COMPILATION_RESULT: CompilationResult = {
            crate::plugins::wasm::compile(PLUGIN_BYTES).unwrap()
        };
    }

    async fn get_initialized_test_environment_with_remote_plugin() -> Result<TestEnvironment, ErrBox> {
        let environment = get_test_environment_with_remote_plugin();
        environment.write_file(&PathBuf::from("./dprint.config.json"), r#"{
            "projectType": "openSource",
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#).unwrap();
        run_test_cli(vec!["--version"], &environment).await.unwrap(); // cause initialization
        environment.clear_logs();
        Ok(environment)
    }

    fn get_test_environment_with_remote_plugin() -> TestEnvironment {
        let environment = TestEnvironment::new();
        environment.add_remote_file("https://plugins.dprint.dev/test-plugin.wasm", PLUGIN_BYTES);
        environment
    }

    pub fn quick_compile(wasm_bytes: &[u8]) -> Result<CompilationResult, ErrBox> {
        if wasm_bytes == PLUGIN_BYTES {
            Ok(COMPILATION_RESULT.clone())
        } else {
            unreachable!()
        }
    }
}
