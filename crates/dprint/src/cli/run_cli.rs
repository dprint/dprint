use std::path::PathBuf;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use colored::Colorize;

use crate::cache::Cache;
use crate::environment::Environment;
use crate::configuration::{self, get_global_config, get_plugin_config_map};
use crate::plugins::{InitializedPlugin, Plugin, PluginResolver, InitializedPluginPool, PluginPools};
use crate::utils::{get_table_text, get_difference, pretty_print_json_text, PathSource, resolve_url_or_file_path_to_path_source};
use crate::types::ErrBox;

use super::{CliArgs, SubCommand, StdInFmt};
use super::configuration::{resolve_config_from_args, ResolvedConfig};

const BOM_CHAR: char = '\u{FEFF}';

pub async fn run_cli<TEnvironment : Environment>(
    args: CliArgs,
    environment: &TEnvironment,
    cache: &Cache<TEnvironment>,
    plugin_resolver: &impl PluginResolver,
    plugin_pools: Arc<PluginPools<TEnvironment>>,
) -> Result<(), ErrBox> {
    // help
    if let SubCommand::Help(help_text) = &args.sub_command {
        return output_help(&args, cache, environment, plugin_resolver, help_text).await;
    }

    // version
    if args.sub_command == SubCommand::Version {
        return output_version(environment).await;
    }

    // license
    if args.sub_command == SubCommand::License {
        return output_license(&args, cache, environment, plugin_resolver).await;
    }

    // editor plugin info
    if args.sub_command == SubCommand::EditorInfo {
        return output_editor_info(&args, cache, environment, plugin_resolver).await;
    }

    // clear cache
    if args.sub_command == SubCommand::ClearCache {
        let cache_dir = environment.get_cache_dir()?; // this actually creates the directory, but whatever
        environment.remove_dir_all(&cache_dir)?;
        environment.log(&format!("Deleted {}", cache_dir.display()));
        return Ok(());
    }

    // init
    if args.sub_command == SubCommand::Init {
        return init_config_file(environment, &args.config).await;
    }

    // get configuration
    let config = resolve_config_from_args(&args, cache, environment).await?;

    // get project type diagnostic, but don't surface any issues yet
    let project_type_result = check_project_type_diagnostic(&config);

    // resolve file paths
    let file_paths = if let SubCommand::StdInFmt(_) = &args.sub_command {
        vec![]
    } else {
        resolve_file_paths(&config, &args, environment)?
    };

    // resolve plugins
    let plugins = resolve_plugins(config, &args, environment, plugin_resolver).await?;
    if plugins.is_empty() {
        return err!("No formatting plugins found. Ensure at least one is specified in the 'plugins' array of the configuration file.");
    }

    // do stdin format
    if let SubCommand::StdInFmt(stdin_fmt) = &args.sub_command {
        return output_stdin_format(&stdin_fmt, environment, plugins);
    }

    // output resolved config
    if args.sub_command == SubCommand::OutputResolvedConfig {
        return output_resolved_config(plugins, environment);
    }

    let format_context = get_format_context(plugins, file_paths);

    // output resolved file paths
    if args.sub_command == SubCommand::OutputFilePaths {
        output_file_paths(format_context.file_paths_by_plugin.values().flat_map(|x| x.iter()), environment);
        return Ok(());
    }

    // error if no file paths
    if format_context.file_paths_by_plugin.is_empty() {
        return err!("No files found to format with the specified plugins. You may want to try using `dprint output-file-paths` to see which files it's finding.");
    }

    // surface the project type error at this point
    project_type_result?;

    // check and format
    if args.sub_command == SubCommand::Check {
        check_files(format_context, environment, plugin_pools).await
    } else if args.sub_command == SubCommand::Fmt {
        format_files(format_context, environment, plugin_pools).await
    } else {
        unreachable!()
    }
}

struct FormatContext {
    pub plugins: Vec<Box<dyn Plugin>>,
    pub file_paths_by_plugin: HashMap<String, Vec<PathBuf>>,
}

fn get_format_context(plugins: Vec<Box<dyn Plugin>>, file_paths: Vec<PathBuf>) -> FormatContext {
    let mut file_paths_by_plugin: HashMap<String, Vec<PathBuf>> = HashMap::new();

    for file_path in file_paths.into_iter() {
        if let Some(file_extension) = crate::utils::get_lowercase_file_extension(&file_path) {
            if let Some(plugin) = plugins.iter().filter(|p| p.file_extensions().contains(&file_extension)).next() {
                if let Some(file_paths) = file_paths_by_plugin.get_mut(plugin.name()) {
                    file_paths.push(file_path);
                } else {
                    file_paths_by_plugin.insert(String::from(plugin.name()), vec![file_path]);
                }
            }
        }
    }

    FormatContext {
        plugins,
        file_paths_by_plugin,
    }
}

async fn output_version<'a, TEnvironment: Environment>(environment: &TEnvironment) -> Result<(), ErrBox> {
    environment.log(&format!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")));

    Ok(())
}

async fn output_help<TEnvironment: Environment>(
    args: &CliArgs,
    cache: &Cache<TEnvironment>,
    environment: &TEnvironment,
    plugin_resolver: &impl PluginResolver,
    help_text: &str,
) -> Result<(), ErrBox> {
    // log the cli's help first
    environment.log(help_text);

    // now check for the plugins
    let plugins_result = get_plugins_from_args(args, cache, environment, plugin_resolver).await;
    match plugins_result {
        Ok(plugins) => {
            if !plugins.is_empty() {
                let plugin_texts = get_table_text(plugins.iter().map(|plugin| (plugin.name(), plugin.help_url())).collect(), 4);
                environment.log("\nPLUGINS HELP:");
                for plugin_text in plugin_texts {
                    // output their names and help urls
                    environment.log(&format!("    {}", plugin_text));
                }
            }
        }
        Err(err) => {
            log_verbose!(environment, "Error getting plugins for help. {:?}", err);
        }
    }

    Ok(())
}

async fn output_license<TEnvironment: Environment>(
    args: &CliArgs,
    cache: &Cache<TEnvironment>,
    environment: &TEnvironment,
    plugin_resolver: &impl PluginResolver
) -> Result<(), ErrBox> {
    environment.log("==== DPRINT CLI LICENSE ====");
    environment.log(std::str::from_utf8(include_bytes!("../../LICENSE"))?);

    // now check for the plugins
    for plugin in get_plugins_from_args(args, cache, environment, plugin_resolver).await? {
        environment.log(&format!("\n==== {} LICENSE ====", plugin.name().to_uppercase()));
        let initialized_plugin = plugin.initialize()?;
        environment.log(&initialized_plugin.get_license_text());
    }

    Ok(())
}

async fn output_editor_info<TEnvironment: Environment>(
    args: &CliArgs,
    cache: &Cache<TEnvironment>,
    environment: &TEnvironment,
    plugin_resolver: &impl PluginResolver
) -> Result<(), ErrBox> {
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct EditorInfo {
        pub schema_version: u32,
        pub plugins: Vec<EditorPluginInfo>,
    }

    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct EditorPluginInfo {
        name: String,
        file_extensions: Vec<String>,
    }

    let mut plugins = Vec::new();

    for plugin in get_plugins_from_args(args, cache, environment, plugin_resolver).await? {
        plugins.push(EditorPluginInfo {
            name: plugin.name().to_string(),
            file_extensions: plugin.file_extensions().iter().map(|ext| ext.to_string()).collect(),
        });
    }

    environment.log_silent(&serde_json::to_string(&EditorInfo {
        schema_version: 1,
        plugins,
    })?);

    Ok(())
}

async fn get_plugins_from_args<TEnvironment : Environment>(
    args: &CliArgs,
    cache: &Cache<TEnvironment>,
    environment: &TEnvironment,
    plugin_resolver: &impl PluginResolver
) -> Result<Vec<Box<dyn Plugin>>, ErrBox> {
    match resolve_config_from_args(args, cache, environment).await {
        Ok(config) => {
            let plugins = resolve_plugins(config, args, environment, plugin_resolver).await?;
            Ok(plugins)
        },
        Err(_) => {
            // ignore
            Ok(Vec::new())
        }
    }
}

fn output_file_paths<'a>(file_paths: impl Iterator<Item=&'a PathBuf>, environment: &impl Environment) {
    for file_path in file_paths {
        environment.log(&file_path.display().to_string())
    }
}

fn output_resolved_config(
    plugins: Vec<Box<dyn Plugin>>,
    environment: &impl Environment,
) -> Result<(), ErrBox> {
    for plugin in plugins {
        let config_key = String::from(plugin.config_key());

        // get an initialized plugin and output its diagnostics
        let initialized_plugin = plugin.initialize()?;
        output_plugin_config_diagnostics(plugin.name(), &initialized_plugin, environment)?;

        let text = initialized_plugin.get_resolved_config();
        let pretty_text = pretty_print_json_text(&text)?;
        environment.log(&format!("{}: {}", config_key, pretty_text));
    }

    Ok(())
}

async fn init_config_file(environment: &impl Environment, config_arg: &Option<String>) -> Result<(), ErrBox> {
    let config_file_path = get_config_path(environment, config_arg)?;
    return if !environment.path_exists(&config_file_path) {
        environment.write_file(&config_file_path, &configuration::get_init_config_file_text(environment).await?)?;
        environment.log(&format!("Created {}", config_file_path.display()));
        Ok(())
    } else {
        err!("Configuration file '{}' already exists.", config_file_path.display())
    };

    fn get_config_path(environment: &impl Environment, config_arg: &Option<String>) -> Result<PathBuf, ErrBox> {
        return Ok(if let Some(config_arg) = config_arg.as_ref() {
            PathBuf::from(config_arg)
        } else if use_config_dir(environment)? {
            PathBuf::from("./config/.dprintrc.json")
        } else {
            PathBuf::from("./.dprintrc.json")
        });

        fn use_config_dir(environment: &impl Environment) -> Result<bool, ErrBox> {
            if environment.path_exists(&PathBuf::from("./config")) {
                let prompt_message = "Would you like to create the .dprintrc.json in the ./config directory?";
                let options = get_table_text(vec![
                    ("Yes", "Create it in the ./config directory."),
                    ("No", "Create it in the current working directory.")
                ], 2);

                Ok(environment.get_selection(prompt_message, &options)? == 0)
            } else {
                Ok(false)
            }
        }
    }
}

fn output_stdin_format<'a, TEnvironment: Environment>(
    stdin_fmt: &StdInFmt,
    environment: &TEnvironment,
    plugins: Vec<Box<dyn Plugin>>,
) -> Result<(), ErrBox> {
    let file_name = PathBuf::from(&stdin_fmt.file_name);
    let ext = match file_name.extension() {
        Some(ext) => ext.to_string_lossy().to_string(),
        None => return err!("Could not find extension for {}", stdin_fmt.file_name),
    };

    for plugin in plugins {
        if plugin.file_extensions().contains(&ext) {
            let initialized_plugin = plugin.initialize()?;
            let result = initialized_plugin.format_text(&file_name, &stdin_fmt.file_text)?;
            environment.log_silent(&result);
            return Ok(());
        }
    }

    err!("Could not find plugin to format the file with extension: {}", ext)
}

async fn check_files<TEnvironment : Environment>(
    format_context: FormatContext,
    environment: &TEnvironment,
    plugin_pools: Arc<PluginPools<TEnvironment>>,
) -> Result<(), ErrBox> {
    let not_formatted_files_count = Arc::new(AtomicUsize::new(0));

    run_parallelized(format_context, environment, plugin_pools, {
        let not_formatted_files_count = not_formatted_files_count.clone();
        move |file_path, file_text, formatted_text, _, environment| {
            if formatted_text != file_text {
                not_formatted_files_count.fetch_add(1, Ordering::SeqCst);
                match get_difference(&file_text, &formatted_text) {
                    Ok(difference_text) => {
                        environment.log(&format!(
                            "{} {}:\n{}\n--",
                            "from".bold().red().to_string(),
                            file_path.display(),
                            difference_text,
                        ));
                    }
                    Err(err) => {
                        environment.log(&format!(
                            "{} {}:\nError getting difference, but this file needs formatting.\n\nError message: {}\n--",
                            "from".bold().red().to_string(),
                            file_path.display(),
                            err.to_string().red().to_string(),
                        ));
                    }
                }
            }
            Ok(None)
        }
    }).await?;

    let not_formatted_files_count = not_formatted_files_count.load(Ordering::SeqCst);
    if not_formatted_files_count == 0 {
        Ok(())
    } else {
        let f = if not_formatted_files_count == 1 { "file" } else { "files" };
        err!("Found {} not formatted {}.", not_formatted_files_count.to_string().bold().to_string(), f)
    }
}

async fn format_files<TEnvironment : Environment>(
    format_context: FormatContext,
    environment: &TEnvironment,
    plugin_pools: Arc<PluginPools<TEnvironment>>,
) -> Result<(), ErrBox> {
    let formatted_files_count = Arc::new(AtomicUsize::new(0));
    let files_count: usize = format_context.file_paths_by_plugin.values().map(|x| x.len()).sum();

    run_parallelized(format_context, environment, plugin_pools, {
        let formatted_files_count = formatted_files_count.clone();
        move |_, file_text, formatted_text, had_bom, _| {
            if formatted_text != file_text {
                let new_text = if had_bom {
                    // add back the BOM
                    format!("{}{}", BOM_CHAR, formatted_text)
                } else {
                    formatted_text
                };

                formatted_files_count.fetch_add(1, Ordering::SeqCst);
                 // todo: use environment.write_file_async here...
                 // It was challenging to figure out how to make this
                 // closure async so I gave up.
                Ok(Some(new_text))
            } else {
                Ok(None)
            }
        }
    }).await?;

    let formatted_files_count = formatted_files_count.load(Ordering::SeqCst);
    if formatted_files_count > 0 {
        let suffix = if files_count == 1 { "file" } else { "files" };
        environment.log(&format!("Formatted {} {}.", formatted_files_count.to_string().bold().to_string(), suffix));
    }

    Ok(())
}

async fn run_parallelized<F, TEnvironment : Environment>(
    format_context: FormatContext,
    environment: &TEnvironment,
    plugin_pools: Arc<PluginPools<TEnvironment>>,
    f: F,
) -> Result<(), ErrBox> where F: Fn(&PathBuf, &str, String, bool, &TEnvironment) -> Result<Option<String>, ErrBox> + Send + 'static + Clone {
    let error_count = Arc::new(AtomicUsize::new(0));
    let plugins = format_context.plugins;
    let file_paths_by_plugin = format_context.file_paths_by_plugin;

    for plugin in plugins.into_iter() {
        plugin_pools.add_plugin(plugin);
    }

    let handles = file_paths_by_plugin.into_iter().map(|(plugin_name, file_paths)| {
        let plugin_pools = plugin_pools.clone();
        let environment = environment.to_owned();
        let f = f.clone();
        let error_count = error_count.clone();
        tokio::task::spawn(async move {
            let result = inner_run(&plugin_name, file_paths, plugin_pools, &environment, f, error_count.clone()).await;
            if let Err(err) = result {
                environment.log_error(&format!("[{}]: {}", plugin_name, err.to_string()));
                error_count.fetch_add(1, Ordering::SeqCst);
            }
        })
    });

    let result = futures::future::try_join_all(handles).await;
    if let Err(err) = result {
        return err!(
            "A panic occurred in a dprint plugin. You may want to run in verbose mode (--verbose) to help figure out where it failed then report this as a bug.\n  Error: {}",
            err.to_string()
        );
    }

    let error_count = error_count.load(Ordering::SeqCst);
    return if error_count == 0 {
        Ok(())
    } else {
        err!("Had {0} error(s) formatting.", error_count)
    };

    #[inline]
    async fn inner_run<F, TEnvironment : Environment>(
        plugin_name: &str,
        file_paths: Vec<PathBuf>,
        plugin_pools: Arc<PluginPools<TEnvironment>>,
        environment: &TEnvironment,
        f: F,
        error_count: Arc<AtomicUsize>,
    ) -> Result<(), ErrBox> where F: Fn(&PathBuf, &str, String, bool, &TEnvironment) -> Result<Option<String>, ErrBox> + Send + 'static + Clone {
        let plugin_pool = plugin_pools.get_pool(plugin_name).expect("Could not get the plugin pool.");

        // get a plugin from the pool then propagate up its configuration diagnostics if necessary
        let plugin = plugin_pool.initialize_first().await?;
        output_plugin_config_diagnostics(&plugin_name, &plugin, environment)?;
        plugin_pool.release(plugin);

        let handles = file_paths.into_iter().map(|file_path| {
            let environment = environment.to_owned();
            let f = f.clone();
            let plugin_pool = plugin_pool.clone();
            let error_count = error_count.clone();

            tokio::task::spawn(async move {
                match run_for_file_path(&file_path, &environment, plugin_pool, f).await {
                    Err(err) => {
                        environment.log_error(&format!("Error formatting {}. Message: {}", file_path.display(), err.to_string()));
                        error_count.fetch_add(1, Ordering::SeqCst);
                    },
                    _ => {}
                }
            })
        }).collect::<Vec<_>>();

        futures::future::try_join_all(handles).await?;

        plugin_pools.release(plugin_name);

        Ok(())
    }

    #[inline]
    async fn run_for_file_path<F, TEnvironment : Environment>(
        file_path: &PathBuf,
        environment: &TEnvironment,
        plugin_pool: Arc<InitializedPluginPool<TEnvironment>>,
        f: F
    ) -> Result<(), ErrBox> where F: Fn(&PathBuf, &str, String, bool, &TEnvironment) -> Result<Option<String>, ErrBox> + Send + 'static + Clone {
        let file_text = environment.read_file_async(&file_path).await?;
        let had_bom = file_text.starts_with(BOM_CHAR);
        let file_text = if had_bom {
            // strip BOM
            &file_text[BOM_CHAR.len_utf8()..]
        } else {
            &file_text
        };

        let formatted_text = {
            // If this returns None, then that means a new instance of a plugin needs
            // to be created for the pool. So this spawns a task creating that pool
            // item, but then goes back to the pool and asks for an instance in case
            // a one has been released in the meantime.
            let initialized_plugin = loop {
                let result = plugin_pool.try_take().await?;
                if let Some(result) = result {
                    break result;
                } else {
                    let plugin_pool = plugin_pool.clone();
                    // todo: any concept of a background task in tokio that would kill
                    // this task on process exit?
                    tokio::task::spawn_blocking(move || {
                        plugin_pool.create_pool_item().expect("Expected to create the plugin.");
                    });
                }
            };

            let start_instant = std::time::Instant::now();
            let format_text_result = initialized_plugin.format_text(file_path, file_text);
            log_verbose!(environment, "Formatted file: {} in {}ms", file_path.display(), start_instant.elapsed().as_millis());
            plugin_pool.release(initialized_plugin);
            format_text_result? // release, then propagate error
        };

        let result = f(&file_path, file_text, formatted_text, had_bom, &environment)?;

        // todo: make the `f` async... couldn't figure it out...
        if let Some(result) = result {
            environment.write_file_async(&file_path, &result).await?;
        }

        Ok(())
    }
}

async fn resolve_plugins(
    config: ResolvedConfig,
    args: &CliArgs,
    environment: &impl Environment,
    plugin_resolver: &impl PluginResolver,
) -> Result<Vec<Box<dyn Plugin>>, ErrBox> {
    let mut config = config;

    // resolve the plugins
    let plugin_path_sources = get_plugin_path_sources(&mut config, args)?;
    let plugins = plugin_resolver.resolve_plugins(plugin_path_sources).await?;

    // resolve each plugin's configuration
    let mut plugins_with_config = Vec::new();
    for plugin in plugins.into_iter() {
        plugins_with_config.push((
            get_plugin_config_map(&plugin, &mut config.config_map)?,
            plugin
        ));
    }

    // now get global config
    let global_config = get_global_config(config.config_map, environment)?;

    // now set each plugin's config
    let mut plugins = Vec::new();
    for (plugin_config, plugin) in plugins_with_config {
        let mut plugin = plugin;
        plugin.set_config(plugin_config, global_config.clone());
        plugins.push(plugin);
    }

    return Ok(plugins);

    fn get_plugin_path_sources<'a>(config: &'a ResolvedConfig, args: &'a CliArgs) -> Result<Vec<PathSource>, ErrBox> {
        let plugin_url_or_file_paths_from_config = &config.plugins;

        Ok(if args.plugins.is_empty() {
            plugin_url_or_file_paths_from_config.clone()
        } else {
            let base_path = PathSource::new_local(config.base_path.clone());
            let mut plugins = Vec::with_capacity(args.plugins.len());
            for url_or_file_path in args.plugins.iter() {
                plugins.push(resolve_url_or_file_path_to_path_source(url_or_file_path, &base_path)?);
            }
            plugins
        })
    }
}

fn check_project_type_diagnostic(config: &ResolvedConfig) -> Result<(), ErrBox> {
    if let Some(diagnostic) = configuration::handle_project_type_diagnostic(&config.project_type) {
        return err!("{}", diagnostic.message);
    }

    Ok(())
}

fn resolve_file_paths(config: &ResolvedConfig, args: &CliArgs, environment: &impl Environment) -> Result<Vec<PathBuf>, ErrBox> {
    let mut file_patterns = get_file_patterns(config, args);
    let absolute_paths = take_absolute_paths(&mut file_patterns);

    let mut file_paths = environment.glob(&config.base_path, &file_patterns)?;
    file_paths.extend(absolute_paths);
    return Ok(file_paths);

    fn get_file_patterns(config: &ResolvedConfig, args: &CliArgs) -> Vec<String> {
        let mut file_patterns = Vec::new();

        file_patterns.extend(if args.file_patterns.is_empty() {
            config.includes.clone() // todo: take from array?
        } else {
            args.file_patterns.clone()
        });

        file_patterns.extend(if args.exclude_file_patterns.is_empty() {
            config.excludes.clone()
        } else {
            args.exclude_file_patterns.clone()
        }.into_iter().map(|exclude| if exclude.starts_with("!") { exclude } else { format!("!{}", exclude) }));

        if !args.allow_node_modules {
            // glob walker will not search the children of a directory once it's ignored like this
            let node_modules_exclude = String::from("!**/node_modules");
            if !file_patterns.contains(&node_modules_exclude) {
                file_patterns.push(node_modules_exclude);
            }
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

        file_patterns
    }

    fn take_absolute_paths(file_patterns: &mut Vec<String>) -> Vec<PathBuf> {
        let len = file_patterns.len();
        let mut file_paths = Vec::new();
        for i in (0..len).rev() {
            if is_absolute_path(&file_patterns[i]) {
                file_paths.push(PathBuf::from(file_patterns.swap_remove(i))); // faster
            }
        }
        file_paths
    }

    fn is_absolute_path(file_pattern: &str) -> bool {
        return !has_glob_chars(file_pattern)
            && PathBuf::from(file_pattern).is_absolute();

        fn has_glob_chars(text: &str) -> bool {
            for c in text.chars() {
                match c {
                    '*' | '{' | '}' | '[' | ']' | '!' => return true,
                    _ => {}
                }
            }

            false
        }
    }
}

fn output_plugin_config_diagnostics(plugin_name: &str, plugin: &Box<dyn InitializedPlugin>, environment: &impl Environment) -> Result<(), ErrBox> {
    let mut diagnostic_count = 0;

    for diagnostic in plugin.get_config_diagnostics() {
        environment.log_error(&format!("[{}]: {}", plugin_name, diagnostic.message));
        diagnostic_count += 1;
    }

    if diagnostic_count > 0 {
        err!("Error initializing from configuration file. Had {} diagnostic(s).", diagnostic_count)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use colored::Colorize;
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;
    use std::sync::Arc;
    use crate::cache::Cache;
    use crate::environment::{Environment, TestEnvironment};
    use crate::configuration::*;
    use crate::plugins::wasm::{WasmPluginResolver, PoolImportObjectFactory};
    use crate::plugins::{PluginPools, PluginCache, CompilationResult};
    use crate::types::ErrBox;
    use crate::utils::get_difference;

    use super::run_cli;
    use super::super::{parse_args, TestStdInReader};

    async fn run_test_cli(args: Vec<&'static str>, environment: &TestEnvironment) -> Result<(), ErrBox> {
        run_test_cli_with_stdin(args, environment, TestStdInReader::new()).await
    }

    async fn run_test_cli_with_stdin(
        args: Vec<&'static str>,
        environment: &TestEnvironment,
        stdin_reader: TestStdInReader, // todo: no clue why this can't be passed in by reference
    ) -> Result<(), ErrBox> {
        let mut args: Vec<String> = args.into_iter().map(String::from).collect();
        args.insert(0, String::from(""));
        let cache = Arc::new(Cache::new(environment.clone()).unwrap());
        let plugin_cache = PluginCache::new(environment.clone(), cache.clone(), &quick_compile);
        let plugin_pools = Arc::new(PluginPools::new(environment.clone()));
        let import_object_factory = PoolImportObjectFactory::new(plugin_pools.clone());
        let plugin_resolver = WasmPluginResolver::new(environment.clone(), plugin_cache, import_object_factory);
        let args = parse_args(args, &stdin_reader)?;
        if args.is_silent_output() { environment.set_silent(); }
        run_cli(args, environment, &cache, &plugin_resolver, plugin_pools).await
    }

    #[tokio::test]
    async fn it_should_output_version_with_no_plugins() {
        let environment = TestEnvironment::new();
        run_test_cli(vec!["--version"], &environment).await.unwrap();
        let logged_messages = environment.get_logged_messages();
        assert_eq!(logged_messages, vec![format!("dprint {}", env!("CARGO_PKG_VERSION"))]);
    }

    #[tokio::test]
    async fn it_should_output_version_with_plugins_but_ignore_them() {
        let environment = get_test_environment_with_remote_plugin();
        environment.write_file(&PathBuf::from("./.dprintrc.json"), r#"{
            "projectType": "openSource",
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#).unwrap();

        run_test_cli(vec!["--version"], &environment).await.unwrap();
        let logged_messages = environment.get_logged_messages();
        assert_eq!(logged_messages, vec![format!("dprint {}", env!("CARGO_PKG_VERSION"))]);
    }

    #[tokio::test]
    async fn it_should_output_help_with_no_plugins() {
        let environment = TestEnvironment::new();
        run_test_cli(vec!["--help"], &environment).await.unwrap();
        let logged_messages = environment.get_logged_messages();
        assert_eq!(logged_messages, vec![get_expected_help_text()]);
    }

    #[tokio::test]
    async fn it_should_output_help_no_sub_commands() {
        let environment = TestEnvironment::new();
        run_test_cli(vec![], &environment).await.unwrap();
        let logged_messages = environment.get_logged_messages();
        assert_eq!(logged_messages, vec![get_expected_help_text()]);
    }

    #[tokio::test]
    async fn it_should_output_help_text_with_plugins() {
        let environment = get_test_environment_with_remote_plugin();
        environment.write_file(&PathBuf::from("./.dprintrc.json"), r#"{
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
        run_test_cli(vec!["output-resolved-config"], &environment).await.unwrap();
        let logged_messages = environment.get_logged_messages();
        assert_eq!(logged_messages, vec!["test-plugin: {\n  \"ending\": \"formatted\",\n  \"lineWidth\": 120\n}"]);
    }

    #[tokio::test]
    async fn it_should_output_resolved_file_paths() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        environment.write_file(&PathBuf::from("/file.txt"), "const t=4;").unwrap();
        environment.write_file(&PathBuf::from("/file2.txt"), "const t=4;").unwrap();
        run_test_cli(vec!["output-file-paths", "**/*.txt"], &environment).await.unwrap();
        let mut logged_messages = environment.get_logged_messages();
        logged_messages.sort();
        assert_eq!(logged_messages, vec!["/file.txt", "/file2.txt"]);
    }

    #[tokio::test]
    async fn it_should_not_output_file_paths_not_supported_by_plugins() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        environment.write_file(&PathBuf::from("/file.ts"), "const t=4;").unwrap();
        environment.write_file(&PathBuf::from("/file2.ts"), "const t=4;").unwrap();
        run_test_cli(vec!["output-file-paths", "**/*.ts"], &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0);
    }

    #[tokio::test]
    async fn it_should_format_files() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        let file_path = PathBuf::from("/file.txt");
        environment.write_file(&file_path, "text").unwrap();
        run_test_cli(vec!["fmt", "/file.txt"], &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.get_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path).unwrap(), "text_formatted");
    }

    #[tokio::test]
    async fn it_should_format_files_with_local_plugin() {
        let environment = get_test_environment_with_local_plugin();
        environment.write_file(&PathBuf::from("./.dprintrc.json"), r#"{
            "projectType": "openSource",
            "plugins": ["/plugins/test-plugin.wasm"]
        }"#).unwrap();
        run_test_cli(vec!["help"], &environment).await.unwrap(); // cause initialization
        environment.clear_logs();
        let file_path = PathBuf::from("/file.txt");
        environment.write_file(&file_path, "text").unwrap();
        run_test_cli(vec!["fmt", "/file.txt"], &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.get_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path).unwrap(), "text_formatted");
    }

    #[tokio::test]
    async fn it_should_handle_plugin_erroring() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        let file_path = PathBuf::from("/file.txt");
        environment.write_file(&file_path, "should_error").unwrap(); // special text that makes the plugin error
        let error_message = run_test_cli(vec!["fmt", "/file.txt"], &environment).await.err().unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0);
        assert_eq!(environment.get_logged_errors(), vec![String::from("Error formatting /file.txt. Message: Did error.")]);
        assert_eq!(error_message.to_string(), "Had 1 error(s) formatting.");
    }

    #[tokio::test]
    async fn it_should_format_calling_other_plugin() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        let file_path = PathBuf::from("/file.txt");
        environment.write_file(&file_path, "plugin: format this text").unwrap();
        run_test_cli(vec!["fmt", "/file.txt"], &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.get_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path).unwrap(), "format this text_formatted");
    }

    #[tokio::test]
    async fn it_should_format_when_specifying_dot_slash_paths() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        let file_path = PathBuf::from("/file.txt");
        environment.write_file(&file_path, "text").unwrap();
        run_test_cli(vec!["fmt", "./file.txt"], &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages(), vec![get_singular_formatted_text()]);
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
        run_test_cli(vec!["fmt", "./**/*.txt", "--excludes", "./file2.txt"], &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages(), vec![get_singular_formatted_text()]);
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
        run_test_cli(vec!["fmt", "**/*.txt"], &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.get_logged_errors().len(), 0);
    }

    #[tokio::test]
    async fn it_should_not_ignore_files_in_node_modules_when_allowed() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        environment.write_file(&PathBuf::from("/node_modules/file.txt"), "const t=4;").unwrap();
        environment.write_file(&PathBuf::from("/test/node_modules/file.txt"), "const t=4;").unwrap();
        run_test_cli(vec!["fmt", "--allow-node-modules", "**/*.txt"], &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages(), vec![get_plural_formatted_text(2)]);
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

        run_test_cli(vec!["fmt", "--config", "/config.json", "/file1.txt", "/file2.txt"], &environment).await.unwrap();

        assert_eq!(environment.get_logged_messages(), vec![get_plural_formatted_text(2)]);
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

        run_test_cli(vec!["fmt", "-c", "/config.json", "/file1.txt"], &environment).await.unwrap();

        assert_eq!(environment.get_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.get_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "text_custom-formatted");
    }

    #[tokio::test]
    async fn it_should_error_when_config_file_does_not_exist() {
        let environment = TestEnvironment::new();
        environment.write_file(&PathBuf::from("/test.txt"), "test").unwrap();

        let error_message = run_test_cli(vec!["fmt", "**/*.txt"], &environment).await.err().unwrap();

        assert_eq!(
            error_message.to_string(),
            concat!(
                "No config file found at ./.dprintrc.json. Did you mean to create (dprint init) or specify one (--config <path>)?\n",
                "  Error: Could not find file at path ./.dprintrc.json"
            )
        );
        assert_eq!(environment.get_logged_messages().len(), 0);
        assert_eq!(environment.get_logged_errors().len(), 0);
    }

    #[tokio::test]
    async fn it_should_support_config_file_urls() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        let file_path1 = PathBuf::from("/file1.txt");
        let file_path2 = PathBuf::from("/file2.txt");
        environment.add_remote_file("https://dprint.dev/test.json", r#"{
            "projectType": "openSource",
            "test-plugin": {
                "ending": "custom-formatted"
            },
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#.as_bytes());
        environment.write_file(&file_path1, "text").unwrap();
        environment.write_file(&file_path2, "text2").unwrap();

        run_test_cli(vec!["fmt", "--config", "https://dprint.dev/test.json", "/file1.txt", "/file2.txt"], &environment).await.unwrap();

        assert_eq!(environment.get_logged_messages(), vec![get_plural_formatted_text(2)]);
        assert_eq!(environment.get_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "text_custom-formatted");
        assert_eq!(environment.read_file(&file_path2).unwrap(), "text2_custom-formatted");
    }

    #[tokio::test]
    async fn it_should_error_on_plugin_config_diagnostic() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        environment.write_file(&PathBuf::from("./.dprintrc.json"), r#"{
            "projectType": "openSource",
            "test-plugin": { "non-existent": 25 },
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#).unwrap();
        environment.write_file(&PathBuf::from("/test.txt"), "test").unwrap();

        let error_message = run_test_cli(vec!["fmt", "**/*.txt"], &environment).await.err().unwrap();

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
        environment.write_file(&PathBuf::from("./.dprintrc.json"), r#"{
            "projectType": "openSource",
            "plugins": []
        }"#).unwrap();
        environment.write_file(&PathBuf::from("/test.txt"), "test").unwrap();

        let error_message = run_test_cli(vec!["fmt", "**/*.txt"], &environment).await.err().unwrap();

        assert_eq!(error_message.to_string(), "No formatting plugins found. Ensure at least one is specified in the 'plugins' array of the configuration file.");
        assert_eq!(environment.get_logged_messages().len(), 0);
        assert_eq!(environment.get_logged_errors().len(), 0);
    }

    #[tokio::test]
    async fn it_should_use_plugins_specified_in_cli_args() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        environment.write_file(&PathBuf::from("./.dprintrc.json"), r#"{
            "projectType": "openSource",
            "plugins": ["https://plugins.dprint.dev/test"]
        }"#).unwrap();
        environment.write_file(&PathBuf::from("/test.txt"), "test").unwrap();

        run_test_cli(vec!["fmt", "**/*.txt", "--plugins", "https://plugins.dprint.dev/test-plugin.wasm"], &environment).await.unwrap();

        assert_eq!(environment.get_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.get_logged_errors().len(), 0);
    }

    #[tokio::test]
    async fn it_should_allow_using_no_config_when_plugins_specified() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        environment.remove_file(&PathBuf::from("./.dprintrc.json")).unwrap();
        environment.write_file(&PathBuf::from("/test.txt"), "test").unwrap();

        run_test_cli(vec!["fmt", "**/*.txt", "--plugins", "https://plugins.dprint.dev/test-plugin.wasm"], &environment).await.unwrap();

        assert_eq!(environment.get_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.get_logged_errors().len(), 0);
    }

    #[tokio::test]
    async fn it_should_error_when_no_files_match_glob() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        let error_message = run_test_cli(vec!["fmt", "**/*.txt"], &environment).await.err().unwrap();

        assert_eq!(
            error_message.to_string(),
            concat!(
                "No files found to format with the specified plugins. ",
                "You may want to try using `dprint output-file-paths` to see which files it's finding."
            )
        );
        assert_eq!(environment.get_logged_messages().len(), 0);
        assert_eq!(environment.get_logged_errors().len(), 0);
    }

    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn it_should_format_absolute_paths_on_windows() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        let file_path = PathBuf::from("E:\\file1.txt");
        environment.set_cwd("D:\\test\\other\\");
        environment.write_file(&file_path, "text1").unwrap();
        environment.write_file(&PathBuf::from("D:\\test\\other\\.dprintrc.json"), r#"{
            "projectType": "openSource",
            "includes": ["**/*.txt"],
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#).unwrap();

        run_test_cli(vec!["fmt", "--", "E:\\file1.txt"], &environment).await.unwrap();

        assert_eq!(environment.get_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.get_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path).unwrap(), "text1_formatted");
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn it_should_format_absolute_paths_on_linux() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        let file_path = PathBuf::from("/asdf/file1.txt");
        environment.set_cwd("/test/other/");
        environment.write_file(&file_path, "text1").unwrap();
        environment.write_file(&PathBuf::from("/test/other/.dprintrc.json"), r#"{
            "projectType": "openSource",
            "includes": ["**/*.txt"],
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#).unwrap();

        run_test_cli(vec!["fmt", "--", "/asdf/file1.txt"], &environment).await.unwrap();

        assert_eq!(environment.get_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.get_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path).unwrap(), "text1_formatted");
    }

    #[tokio::test]
    async fn it_should_format_files_with_config_includes() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        let file_path1 = PathBuf::from("/file1.txt");
        let file_path2 = PathBuf::from("/file2.txt");
        environment.write_file(&file_path1, "text1").unwrap();
        environment.write_file(&file_path2, "text2").unwrap();
        environment.write_file(&PathBuf::from("./.dprintrc.json"), r#"{
            "projectType": "openSource",
            "includes": ["**/*.txt"],
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#).unwrap();

        run_test_cli(vec!["fmt"], &environment).await.unwrap();

        assert_eq!(environment.get_logged_messages(), vec![get_plural_formatted_text(2)]);
        assert_eq!(environment.get_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "text1_formatted");
        assert_eq!(environment.read_file(&file_path2).unwrap(), "text2_formatted");
    }

    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn it_should_format_files_with_config_includes_when_using_back_slashes() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        let file_path1 = PathBuf::from("/file1.txt");
        environment.write_file(&file_path1, "text1").unwrap();
        environment.write_file(&PathBuf::from("./.dprintrc.json"), r#"{
            "projectType": "openSource",
            "includes": ["**\\*.txt"],
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#).unwrap();

        run_test_cli(vec!["fmt"], &environment).await.unwrap();

        assert_eq!(environment.get_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.get_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "text1_formatted");
    }

    #[tokio::test]
    async fn it_should_override_config_includes_with_cli_includes() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        let file_path1 = PathBuf::from("/file1.txt");
        let file_path2 = PathBuf::from("/file2.txt");
        environment.write_file(&file_path1, "text1").unwrap();
        environment.write_file(&file_path2, "text2").unwrap();
        environment.write_file(&PathBuf::from("./.dprintrc.json"), r#"{
            "projectType": "openSource",
            "includes": ["**/*.txt"],
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#).unwrap();

        run_test_cli(vec!["fmt", "/file1.txt"], &environment).await.unwrap();

        assert_eq!(environment.get_logged_messages(), vec![get_singular_formatted_text()]);
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
        environment.write_file(&PathBuf::from("./.dprintrc.json"), r#"{
            "projectType": "openSource",
            "includes": ["**/*.txt"],
            "excludes": ["/file1.txt"],
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#).unwrap();

        run_test_cli(vec!["fmt", "--excludes", "/file2.txt"], &environment).await.unwrap();

        assert_eq!(environment.get_logged_messages(), vec![get_singular_formatted_text()]);
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
        environment.write_file(&PathBuf::from("./.dprintrc.json"), r#"{
            "projectType": "openSource",
            "includes": ["/file2.txt"],
            "excludes": ["/file1.txt"],
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#).unwrap();

        run_test_cli(vec!["fmt", "/file1.txt", "--excludes", "/file2.txt"], &environment).await.unwrap();

        assert_eq!(environment.get_logged_messages(), vec![get_singular_formatted_text()]);
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
        environment.write_file(&PathBuf::from("./.dprintrc.json"), r#"{
            "projectType": "openSource",
            "includes": ["**/*.txt"],
            "excludes": ["/file2.txt"],
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#).unwrap();

        run_test_cli(vec!["fmt"], &environment).await.unwrap();

        assert_eq!(environment.get_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.get_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "text1_formatted");
        assert_eq!(environment.read_file(&file_path2).unwrap(), "text2");
    }

    #[tokio::test]
    async fn it_should_format_files_with_config_in_config_sub_dir() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        environment.remove_file(&PathBuf::from("./.dprintrc.json")).unwrap();
        let file_path1 = PathBuf::from("/file1.txt");
        let file_path2 = PathBuf::from("/file2.txt");
        environment.write_file(&file_path1, "text1").unwrap();
        environment.write_file(&file_path2, "text2").unwrap();
        environment.write_file(&PathBuf::from("./config/.dprintrc.json"), r#"{
            "projectType": "openSource",
            "includes": ["**/*.txt"],
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#).unwrap();

        run_test_cli(vec!["fmt"], &environment).await.unwrap();

        assert_eq!(environment.get_logged_messages(), vec![get_plural_formatted_text(2)]);
        assert_eq!(environment.get_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "text1_formatted");
        assert_eq!(environment.read_file(&file_path2).unwrap(), "text2_formatted");
    }

    #[tokio::test]
    async fn it_should_format_using_config_in_ancestor_directory() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        environment.write_file(&PathBuf::from("./.dprintrc.json"), r#"{
            "projectType": "openSource",
            "includes": ["**/*.txt"],
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#).unwrap();
        environment.set_cwd("/test/other/");
        let file_path = PathBuf::from("/test/other/file.txt");
        environment.write_file(&file_path, "text").unwrap();
        run_test_cli(vec!["fmt"], &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.get_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path).unwrap(), "text_formatted");
    }

    #[tokio::test]
    async fn it_should_format_using_config_in_ancestor_directory_config_folder() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        environment.remove_file(&PathBuf::from("./.dprintrc.json")).unwrap();
        environment.write_file(&PathBuf::from("./config/.dprintrc.json"), r#"{
            "projectType": "openSource",
            "includes": ["**/*.txt"],
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#).unwrap();
        environment.set_cwd("/test/other/");
        let file_path = PathBuf::from("/test/other/file.txt");
        environment.write_file(&file_path, "text").unwrap();
        run_test_cli(vec!["fmt"], &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.get_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path).unwrap(), "text_formatted");
    }

    #[tokio::test]
    async fn it_should_error_when_missing_project_type() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        environment.write_file(&PathBuf::from("./.dprintrc.json"), r#"{
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#).unwrap();
        environment.write_file(&PathBuf::from("/file1.txt"), "text1_formatted").unwrap();
        let error_message = run_test_cli(vec!["fmt", "/file1.txt"], &environment).await.err().unwrap();
        assert_eq!(error_message.to_string().find("The 'projectType' property").is_some(), true);
        assert_eq!(environment.get_logged_messages().len(), 0);
        assert_eq!(environment.get_logged_errors().len(), 0);
    }

    #[tokio::test]
    async fn it_should_not_output_when_no_files_need_formatting() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        environment.write_file(&PathBuf::from("/file.txt"), "text_formatted").unwrap();
        run_test_cli(vec!["fmt", "/file.txt"], &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0);
        assert_eq!(environment.get_logged_errors().len(), 0);
    }

    #[tokio::test]
    async fn it_should_not_output_when_no_files_need_formatting_for_check() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        let file_path = PathBuf::from("/file.txt");
        environment.write_file(&file_path, "text_formatted").unwrap();
        run_test_cli(vec!["check", "/file.txt"], &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages().len(), 0);
        assert_eq!(environment.get_logged_errors().len(), 0);
    }

    #[tokio::test]
    async fn it_should_output_when_a_file_need_formatting_for_check() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        environment.write_file(&PathBuf::from("/file.txt"), "const t=4;").unwrap();
        let error_message = run_test_cli(vec!["check", "/file.txt"], &environment).await.err().unwrap();
        assert_eq!(error_message.to_string(), get_singular_check_text());
        assert_eq!(environment.get_logged_messages(), vec![
            format!(
                "{}\n{}\n--",
                format!("{} /file.txt:", "from".bold().red().to_string()),
                get_difference("const t=4;", "const t=4;_formatted").unwrap(),
            ),
        ]);
        assert_eq!(environment.get_logged_errors().len(), 0);
    }

    #[tokio::test]
    async fn it_should_output_when_files_need_formatting_for_check() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        environment.write_file(&PathBuf::from("/file1.txt"), "const t=4;").unwrap();
        environment.write_file(&PathBuf::from("/file2.txt"), "const t=5;").unwrap();

        let error_message = run_test_cli(vec!["check", "/file1.txt", "/file2.txt"], &environment).await.err().unwrap();
        assert_eq!(error_message.to_string(), get_plural_check_text(2));
        let mut logged_messages = environment.get_logged_messages();
        logged_messages.sort(); // seems like the order is not deterministic
        assert_eq!(logged_messages, vec![
            format!(
                "{}\n{}\n--",
                format!("{} /file1.txt:", "from".bold().red().to_string()),
                get_difference("const t=4;", "const t=4;_formatted").unwrap(),
            ),
            format!(
                "{}\n{}\n--",
                format!("{} /file2.txt:", "from".bold().red().to_string()),
                get_difference("const t=5;", "const t=5;_formatted").unwrap(),
            ),
        ]);
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
                "configKey": "typescript",
                "configExcludes": []
            }, {
                "name": "dprint-plugin-jsonc",
                "version": "0.2.3",
                "url": "https://plugins.dprint.dev/json-0.2.3.wasm",
                "configKey": "json",
                "configExcludes": []
            }]
        }"#.as_bytes());
        let expected_text = get_init_config_file_text(&environment).await.unwrap();
        environment.clear_logs();
        run_test_cli(vec!["init"], &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages(), vec![
            "What kind of project will dprint be formatting?\n\nSee commercial pricing at: https://dprint.dev/pricing\n",
            "Created ./.dprintrc.json"
        ]);
        assert_eq!(environment.read_file(&PathBuf::from("./.dprintrc.json")).unwrap(), expected_text);
    }

    #[tokio::test]
    async fn it_should_initialize_with_specified_config_path() {
        let environment = TestEnvironment::new();
        environment.add_remote_file(crate::plugins::REMOTE_INFO_URL, r#"{
            "schemaVersion": 1,
            "pluginSystemSchemaVersion": 1,
            "latest": [{
                "name": "dprint-plugin-typescript",
                "version": "0.17.2",
                "url": "https://plugins.dprint.dev/typescript-0.17.2.wasm",
                "configKey": "typescript",
                "configExcludes": []
            }]
        }"#.as_bytes());
        let expected_text = get_init_config_file_text(&environment).await.unwrap();
        environment.clear_logs();
        run_test_cli(vec!["init", "--config", "./test.config.json"], &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages(), vec![
            "What kind of project will dprint be formatting?\n\nSee commercial pricing at: https://dprint.dev/pricing\n",
            "Created ./test.config.json"
        ]);
        assert_eq!(environment.read_file(&PathBuf::from("./test.config.json")).unwrap(), expected_text);
    }

    #[tokio::test]
    async fn it_should_error_when_config_file_exists_on_initialize() {
        let environment = TestEnvironment::new();
        environment.write_file(&PathBuf::from("./.dprintrc.json"), "{}").unwrap();
        let error_message = run_test_cli(vec!["init"], &environment).await.err().unwrap();
        assert_eq!(error_message.to_string(), "Configuration file './.dprintrc.json' already exists.");
    }

    #[tokio::test]
    async fn it_should_ask_to_initialize_in_config_dir() {
        let environment = TestEnvironment::new();
        environment.write_file(&PathBuf::from("./config"), "").unwrap(); // hack for creating a directory with the test environment...
        let expected_text = get_init_config_file_text(&environment).await.unwrap();
        environment.clear_logs();
        run_test_cli(vec!["init"], &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages(), vec![
            "Would you like to create the .dprintrc.json in the ./config directory?",
            "What kind of project will dprint be formatting?\n\nSee commercial pricing at: https://dprint.dev/pricing\n",
            "Created ./config/.dprintrc.json"
        ]);
        assert_eq!(environment.read_file(&PathBuf::from("./config/.dprintrc.json")).unwrap(), expected_text);
    }

    #[tokio::test]
    async fn it_should_ask_to_initialize_in_config_dir_and_handle_no() {
        let environment = TestEnvironment::new();
        environment.write_file(&PathBuf::from("./config"), "").unwrap(); // hack for creating a directory with the test environment...
        environment.set_selection_result(1);
        let expected_text = get_init_config_file_text(&environment).await.unwrap();
        environment.clear_logs();
        run_test_cli(vec!["init"], &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages(), vec![
            "Would you like to create the .dprintrc.json in the ./config directory?",
            "What kind of project will dprint be formatting?\n\nSee commercial pricing at: https://dprint.dev/pricing\n",
            "Created ./.dprintrc.json"
        ]);
        assert_eq!(environment.read_file(&PathBuf::from("./.dprintrc.json")).unwrap(), expected_text);
    }

    #[tokio::test]
    async fn it_should_clear_cache_directory() {
        let environment = TestEnvironment::new();
        run_test_cli(vec!["clear-cache"], &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages(), vec!["Deleted /cache"]);
        assert_eq!(environment.is_dir_deleted(&PathBuf::from("/cache")), true);
    }

    #[tokio::test]
    async fn it_should_handle_bom() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        let file_path = PathBuf::from("/file.txt");
        environment.write_file(&file_path, "\u{FEFF}text").unwrap();
        run_test_cli(vec!["fmt", "/file.txt"], &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.get_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path).unwrap(), "\u{FEFF}text_formatted");
    }

    #[tokio::test]
    async fn it_should_output_license_for_sub_command_with_no_plugins() {
        let environment = TestEnvironment::new();
        run_test_cli(vec!["license"], &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages(), vec![
            "==== DPRINT CLI LICENSE ====",
            std::str::from_utf8(include_bytes!("../../LICENSE")).unwrap()
        ]);
    }

    #[tokio::test]
    async fn it_should_output_license_for_sub_command_with_plugins() {
        let environment = get_initialized_test_environment_with_remote_plugin().await.unwrap();
        run_test_cli(vec!["license"], &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages(), vec![
            "==== DPRINT CLI LICENSE ====",
            std::str::from_utf8(include_bytes!("../../LICENSE")).unwrap(),
            "\n==== TEST-PLUGIN LICENSE ====",
            r#"Copyright 2020 David Sherret. All rights reserved.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
"#
        ]);
    }

    #[tokio::test]
    async fn it_should_output_editor_plugin_info() {
        // it should not output anything when downloading plugins
        let environment = get_test_environment_with_remote_plugin();
        environment.write_file(&PathBuf::from("./.dprintrc.json"), r#"{
            "projectType": "openSource",
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#).unwrap();
        run_test_cli(vec!["editor-info"], &environment).await.unwrap();
        assert_eq!(environment.get_logged_messages(), vec![
            r#"{"schemaVersion":1,"plugins":[{"name":"test-plugin","fileExtensions":["txt"]}]}"#
        ]);
    }

    #[tokio::test]
    async fn it_should_format_for_stdin() {
        // it should not output anything when downloading plugins
        let environment = get_test_environment_with_remote_plugin();
        environment.write_file(&PathBuf::from("./.dprintrc.json"), r#"{
            "projectType": "openSource",
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#).unwrap();
        let test_std_in = TestStdInReader::new_with_text("text");
        run_test_cli_with_stdin(vec!["stdin-fmt", "--file-name", "file.txt"], &environment, test_std_in).await.unwrap();
        assert_eq!(environment.get_logged_messages(), vec!["text_formatted"]);
        assert_eq!(environment.get_logged_errors().len(), 0);
    }

    #[tokio::test]
    async fn it_should_error_when_format_for_stdin_no_matching_extension() {
        // it should not output anything when downloading plugins
        let environment = get_test_environment_with_remote_plugin();
        environment.write_file(&PathBuf::from("./.dprintrc.json"), r#"{
            "projectType": "openSource",
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#).unwrap();
        let test_std_in = TestStdInReader::new_with_text("text");
        let error_message = run_test_cli_with_stdin(vec!["stdin-fmt", "--file-name", "file.ts"], &environment, test_std_in).await.err().unwrap();
        assert_eq!(error_message.to_string(), "Could not find plugin to format the file with extension: ts");
    }

    fn get_singular_formatted_text() -> String {
        format!("Formatted {} file.", "1".bold().to_string())
    }

    fn get_plural_formatted_text(count: usize) -> String {
        format!("Formatted {} files.", count.to_string().bold().to_string())
    }

    fn get_singular_check_text() -> String {
        format!("Found {} not formatted file.", "1".bold().to_string())
    }

    fn get_plural_check_text(count: usize) -> String {
        format!("Found {} not formatted files.", count.to_string().bold().to_string())
    }

    fn get_expected_help_text() -> &'static str {
        concat!("dprint ", env!("CARGO_PKG_VERSION"), r#"
Copyright 2020 by David Sherret

Auto-formats source code based on the specified plugins.

USAGE:
    dprint <SUBCOMMAND> [OPTIONS] [--] [files]...

SUBCOMMANDS:
    init                      Initializes a configuration file in the current directory.
    fmt                       Formats the source files and writes the result to the file system.
    check                     Checks for any files that haven't been formatted.
    output-file-paths         Prints the resolved file paths for the plugins based on the args and configuration.
    output-resolved-config    Prints the resolved configuration for the plugins based on the args and configuration.
    clear-cache               Deletes the plugin cache directory.
    license                   Outputs the software license.

OPTIONS:
    -c, --config <config>            Path or url to JSON configuration file. Defaults to .dprintrc.json in current or
                                     ancestor directory when not provided.
        --excludes <patterns>...     List of files or directories or globs in quotes to exclude when formatting. This
                                     overrides what is specified in the config file.
        --allow-node-modules         Allows traversing node module directories (unstable - This flag will be renamed to
                                     be non-node specific in the future).
        --plugins <urls/files>...    List of urls or file paths of plugins to use. This overrides what is specified in
                                     the config file.
        --verbose                    Prints additional diagnostic information.
    -v, --version                    Prints the version.

ARGS:
    <files>...    List of files or globs in quotes to format. This overrides what is specified in the config file.

GETTING STARTED:
    1. Navigate to the root directory of a code repository.
    2. Run `dprint init` to create a .dprintrc.json file in that directory.
    3. Modify configuration file if necessary.
    4. Run `dprint fmt` or `dprint check`.

EXAMPLES:
    Write formatted files to file system:

      dprint fmt

    Check for files that haven't been formatted:

      dprint check

    Specify path to config file other than the default:

      dprint fmt --config path/to/config/.dprintrc.json

    Search for files using the specified file patterns:

      dprint fmt "**/*.{ts,tsx,js,jsx,json}""#)
    }

    // If this file doesn't exist, run `./build.sh` in /crates/test-plugin. (Please consider helping me do something better here :))
    static PLUGIN_BYTES: &'static [u8] = include_bytes!("../../../test-plugin/target/wasm32-unknown-unknown/release/test_plugin.wasm");
    lazy_static! {
        // cache the compilation so this only has to be done once across all tests
        static ref COMPILATION_RESULT: CompilationResult = {
            crate::plugins::wasm::compile(PLUGIN_BYTES).unwrap()
        };
    }

    async fn get_initialized_test_environment_with_remote_plugin() -> Result<TestEnvironment, ErrBox> {
        let environment = get_test_environment_with_remote_plugin();
        environment.write_file(&PathBuf::from("./.dprintrc.json"), r#"{
            "projectType": "openSource",
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#).unwrap();
        run_test_cli(vec!["help"], &environment).await.unwrap(); // cause initialization
        environment.clear_logs();
        Ok(environment)
    }

    fn get_test_environment_with_remote_plugin() -> TestEnvironment {
        let environment = TestEnvironment::new();
        environment.add_remote_file("https://plugins.dprint.dev/test-plugin.wasm", PLUGIN_BYTES);
        environment
    }

    fn get_test_environment_with_local_plugin() -> TestEnvironment {
        let environment = TestEnvironment::new();
        environment.write_file_bytes(&PathBuf::from("/plugins/test-plugin.wasm"), PLUGIN_BYTES).unwrap();
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
