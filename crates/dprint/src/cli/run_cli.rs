use crate::utils::to_absolute_glob;
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use globset::{Glob, GlobSet, GlobSetBuilder};
use parking_lot::Mutex;
use std::time::Instant;
use crossterm::style::Stylize;
use dprint_core::types::ErrBox;

use crate::cache::{Cache, CreateCacheItemOptions};
use crate::environment::Environment;
use crate::configuration::{self, get_global_config, get_plugin_config_map};
use crate::plugins::{InitializedPlugin, InitializedPluginPool, Plugin, PluginResolver, PluginPools, do_batch_format,
    output_plugin_config_diagnostics, TakePluginResult};
use crate::utils::{get_table_text, get_difference, pretty_print_json_text, FileText, BOM_CHAR, ErrorCountLogger};

use super::{CliArgs, SubCommand, EditorServiceSubCommand};
use super::configuration::{resolve_config_from_args, ResolvedConfig};
use super::incremental::IncrementalFile;

// TODO: probably a lot of these functions could be moved into new files

pub fn run_cli<TEnvironment: Environment>(
    args: CliArgs,
    environment: &TEnvironment,
    cache: &Cache<TEnvironment>,
    plugin_resolver: &PluginResolver<TEnvironment>,
    plugin_pools: Arc<PluginPools<TEnvironment>>,
) -> Result<(), ErrBox> {
    // todo: reduce code duplication in this function
    match &args.sub_command {
        SubCommand::Help(help_text) => output_help(&args, cache, environment, plugin_resolver, help_text),
        SubCommand::License => output_license(&args, cache, environment, plugin_resolver),
        SubCommand::EditorInfo => output_editor_info(&args, cache, environment, plugin_resolver),
        SubCommand::EditorService(cmd) => run_editor_service(&args, cache, environment, plugin_resolver, plugin_pools, cmd),
        SubCommand::ClearCache => clear_cache(environment),
        SubCommand::Init => init_config_file(environment, &args.config),
        SubCommand::Version => output_version(environment),
        SubCommand::StdInFmt(cmd) => {
            let config = resolve_config_from_args(&args, cache, environment)?;
            let plugins = resolve_plugins_and_err_if_empty(&config, environment, plugin_resolver)?;
            plugin_pools.set_plugins(plugins);
            // if the path is absolute, then apply exclusion rules
            if environment.is_absolute_path(&cmd.file_path) {
                let file_paths = get_and_resolve_file_paths(&config, &args, environment)?;
                // canonicalize the file path, then check if it's in the list of file paths.
                match environment.canonicalize(&cmd.file_path) {
                    Ok(resolved_file_path) => {
                        // log the file text as-is since it's not in the list of files to format
                        if !file_paths.contains(&resolved_file_path) {
                            environment.log_silent(&cmd.file_text);
                            return Ok(());
                        }
                    }
                    Err(err) => return err!("Error canonicalizing file {}: {}", cmd.file_path.display(), err.to_string()),
                }
            }
            output_stdin_format(&cmd.file_path, &cmd.file_text, environment, plugin_pools)
        }
        SubCommand::OutputResolvedConfig => {
            let config = resolve_config_from_args(&args, cache, environment)?;
            let plugins = resolve_plugins_and_err_if_empty(&config, environment, plugin_resolver)?;
            output_resolved_config(plugins, environment)
        }
        SubCommand::OutputFilePaths => {
            let config = resolve_config_from_args(&args, cache, environment)?;
            let plugins = resolve_plugins_and_err_if_empty(&config, environment, plugin_resolver)?;
            let file_paths = get_and_resolve_file_paths(&config, &args, environment)?;
            let file_paths_by_plugin = get_file_paths_by_plugin(&plugins, file_paths);
            output_file_paths(file_paths_by_plugin.values().flat_map(|x| x.iter()), environment);
            Ok(())
        }
        SubCommand::OutputFormatTimes => {
            let config = resolve_config_from_args(&args, cache, environment)?;
            let plugins = resolve_plugins_and_err_if_empty(&config, environment, plugin_resolver)?;
            let file_paths = get_and_resolve_file_paths(&config, &args, environment)?;
            let file_paths_by_plugin = get_file_paths_by_plugin_and_err_if_empty(&plugins, file_paths)?;
            plugin_pools.set_plugins(plugins);
            output_format_times(file_paths_by_plugin, environment, plugin_pools)
        }
        SubCommand::Check => {
            let config = resolve_config_from_args(&args, cache, environment)?;
            let plugins = resolve_plugins_and_err_if_empty(&config, environment, plugin_resolver)?;
            let file_paths = get_and_resolve_file_paths(&config, &args, environment)?;
            let file_paths_by_plugin = get_file_paths_by_plugin_and_err_if_empty(&plugins, file_paths)?;
            plugin_pools.set_plugins(plugins);

            let incremental_file = get_incremental_file(&args, &config, &cache, &plugin_pools, &environment);
            check_files(file_paths_by_plugin, environment, plugin_pools, incremental_file)
        }
        SubCommand::Fmt => {
            let config = resolve_config_from_args(&args, cache, environment)?;
            let plugins = resolve_plugins_and_err_if_empty(&config, environment, plugin_resolver)?;
            let file_paths = get_and_resolve_file_paths(&config, &args, environment)?;
            let file_paths_by_plugin = get_file_paths_by_plugin_and_err_if_empty(&plugins, file_paths)?;
            plugin_pools.set_plugins(plugins);

            let incremental_file = get_incremental_file(&args, &config, &cache, &plugin_pools, &environment);
            format_files(file_paths_by_plugin, environment, plugin_pools, incremental_file)
        }
        #[cfg(target_os = "windows")]
        SubCommand::Hidden(hidden_command) => {
            match hidden_command {
                super::HiddenSubCommand::WindowsInstall(install_path) => super::install::handle_windows_install(environment, &install_path),
                super::HiddenSubCommand::WindowsUninstall(install_path) => super::install::handle_windows_uninstall(environment, &install_path),
            }
        }
    }
}

fn get_file_paths_by_plugin_and_err_if_empty(
    plugins: &Vec<Box<dyn Plugin>>,
    file_paths: Vec<PathBuf>
) -> Result<HashMap<String, Vec<PathBuf>>, ErrBox> {
    let file_paths_by_plugin = get_file_paths_by_plugin(plugins, file_paths);
    if file_paths_by_plugin.is_empty() {
        return err!("No files found to format with the specified plugins. You may want to try using `dprint output-file-paths` to see which files it's finding.");
    }
    Ok(file_paths_by_plugin)
}

fn get_file_paths_by_plugin(plugins: &Vec<Box<dyn Plugin>>, file_paths: Vec<PathBuf>) -> HashMap<String, Vec<PathBuf>> {
    let mut plugin_by_file_extension: HashMap<&str, &str> = HashMap::new();
    let mut plugin_by_file_name: HashMap<&str, &str> = HashMap::new();

    for plugin in plugins.iter() {
        for file_extension in plugin.file_extensions() {
            plugin_by_file_extension.entry(file_extension).or_insert(plugin.name());
        }
        for file_name in plugin.file_names() {
            plugin_by_file_name.entry(file_name).or_insert(plugin.name());
        }
    }

    let mut file_paths_by_plugin: HashMap<String, Vec<PathBuf>> = HashMap::new();

    for file_path in file_paths.into_iter() {
        let plugin = if let Some(plugin) = crate::utils::get_lowercase_file_name(&file_path).and_then(|k| plugin_by_file_name.get(k.as_str())) {
            plugin
        } else if let Some(plugin) = crate::utils::get_lowercase_file_extension(&file_path).and_then(|k| plugin_by_file_extension.get(k.as_str())) {
            plugin
        } else {
            continue;
        };
        let file_paths = file_paths_by_plugin.entry(plugin.to_string()).or_insert(vec![]);
        file_paths.push(file_path);
    }

    file_paths_by_plugin
}

fn output_version<'a, TEnvironment: Environment>(environment: &TEnvironment) -> Result<(), ErrBox> {
    environment.log(&format!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")));

    Ok(())
}

fn output_help<TEnvironment: Environment>(
    args: &CliArgs,
    cache: &Cache<TEnvironment>,
    environment: &TEnvironment,
    plugin_resolver: &PluginResolver<TEnvironment>,
    help_text: &str,
) -> Result<(), ErrBox> {
    // log the cli's help first
    environment.log(help_text);

    // now check for the plugins
    let plugins_result = get_plugins_from_args(args, cache, environment, plugin_resolver);
    match plugins_result {
        Ok(plugins) => {
            if !plugins.is_empty() {
                let table_text = get_table_text(plugins.iter().map(|plugin| (plugin.name(), plugin.help_url())).collect());
                environment.log("\nPLUGINS HELP:");
                environment.log(&table_text.render(
                    4, // indent
                    // don't render taking terminal width into account
                    // as these are urls and we want them to be clickable
                    None,
                ));
            }
        }
        Err(err) => {
            log_verbose!(environment, "Error getting plugins for help. {}", err.to_string());
        }
    }

    Ok(())
}

fn output_license<TEnvironment: Environment>(
    args: &CliArgs,
    cache: &Cache<TEnvironment>,
    environment: &TEnvironment,
    plugin_resolver: &PluginResolver<TEnvironment>,
) -> Result<(), ErrBox> {
    environment.log("==== DPRINT CLI LICENSE ====");
    environment.log(std::str::from_utf8(include_bytes!("../../LICENSE"))?);

    // now check for the plugins
    for plugin in get_plugins_from_args(args, cache, environment, plugin_resolver)? {
        environment.log(&format!("\n==== {} LICENSE ====", plugin.name().to_uppercase()));
        let initialized_plugin = plugin.initialize()?;
        environment.log(&initialized_plugin.get_license_text()?);
    }

    Ok(())
}

fn output_editor_info<TEnvironment: Environment>(
    args: &CliArgs,
    cache: &Cache<TEnvironment>,
    environment: &TEnvironment,
    plugin_resolver: &PluginResolver<TEnvironment>,
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
        #[serde(default = "Vec::new")]
        file_names: Vec<String>,
    }

    let mut plugins = Vec::new();

    for plugin in get_plugins_from_args(args, cache, environment, plugin_resolver)? {
        plugins.push(EditorPluginInfo {
            name: plugin.name().to_string(),
            file_extensions: plugin.file_extensions().iter().map(|ext| ext.to_string()).collect(),
            file_names: plugin.file_names().iter().map(|ext| ext.to_string()).collect(),
        });
    }

    environment.log_silent(&serde_json::to_string(&EditorInfo {
        schema_version: 3,
        plugins,
    })?);

    Ok(())
}

fn build_glob_set<TEnvironment: Environment>(
    config: &ResolvedConfig,
    environment: &TEnvironment,
    file_patterns: &mut Vec<String>,
) -> Result<GlobSet, ErrBox> {
    let mut builder = GlobSetBuilder::new();
    let cwd = environment.cwd()?;
    let config_base_path = config.base_path.to_string_lossy();
    let base_path = match config_base_path.as_ref() {
        "./" => cwd.to_string_lossy(),
        _ => config_base_path
    };
    let absolute_paths = take_absolute_paths(file_patterns, environment);
    for pattern in to_absolute_globs(file_patterns, &base_path) {
        builder.add(Glob::new(&pattern)?);
    }
    for path in absolute_paths {
        let path_as_str = path.to_string_lossy();
        builder.add(Glob::new(&path_as_str)?);
    }
    return Ok(builder.build().unwrap());
}

fn build_include_exclude_glob_sets<TEnvironment: Environment>(
    config: &ResolvedConfig,
    args: &CliArgs,
    environment: &TEnvironment,
) -> Result<(GlobSet, GlobSet), ErrBox> {
    let cwd = environment.cwd()?;
    let cwd_str = cwd.to_string_lossy();
    let mut include_file_patterns = get_include_file_patterns(&config, args, &cwd_str);
    let include_globset = build_glob_set(config, environment, &mut include_file_patterns)?;

    let mut exclude_file_patterns = get_exclude_file_patterns(&config, args, &cwd_str);
    let exclude_globset = build_glob_set(config, environment, &mut exclude_file_patterns)?;

    return Ok((include_globset, exclude_globset));
}

fn run_editor_service<TEnvironment: Environment>(
    args: &CliArgs,
    cache: &Cache<TEnvironment>,
    environment: &TEnvironment,
    plugin_resolver: &PluginResolver<TEnvironment>,
    plugin_pools: Arc<PluginPools<TEnvironment>>,
    editor_service_cmd: &EditorServiceSubCommand,
) -> Result<(), ErrBox> {
    use dprint_core::plugins::process::{StdIoReaderWriter, StdIoMessenger, start_parent_process_checker_thread};

    // poll for the existence of the parent process and terminate this process when that process no longer exists
    let _handle = start_parent_process_checker_thread(editor_service_cmd.parent_pid);

    let stdin = environment.stdin();
    let stdout = environment.stdout();
    let reader_writer = StdIoReaderWriter::new(stdin, stdout);
    let mut messenger = StdIoMessenger::new(reader_writer);
    let mut past_config: Option<ResolvedConfig> = None;

    loop {
        let message_kind = messenger.read_code()?;
        match message_kind {
            // shutdown
            0 => return Ok(()),
            // check path
            1 => {
                let file_path = messenger.read_single_part_path_buf_message()?;
                // update the glob file patterns and absolute paths by re-retrieving the config file
                let config = resolve_config_from_args(args, cache, environment)?;
                let (include_globset, exclude_globset) = build_include_exclude_glob_sets(&config, args, environment)?;

                // canonicalize the file path, then check if it's in the list of file paths.
                match environment.canonicalize(&file_path) {
                    Ok(resolved_file_path) => {
                        let matches_includes = include_globset.is_match(&resolved_file_path);
                        let matches_excludes = exclude_globset.is_match(&resolved_file_path);
                        let file_exists = environment.path_exists(PathBuf::new().join(config.base_path).join(file_path).as_path());
                        messenger.send_message(if file_exists && matches_includes && !matches_excludes { 1 } else { 0 }, Vec::new())?;
                    },
                    Err(err) => {
                        environment.log_error(&format!("Error canonicalizing file {}: {}", file_path.display(), err.to_string()));
                        messenger.send_message(0, Vec::new())?; // don't format, something went wrong
                    }
                }
            },
            // format
            2 => {
                let mut parts = messenger.read_multi_part_message(2)?;
                let file_path = parts.take_path_buf()?;
                let file_text = parts.take_string()?;

                let result = format_text(args, cache, environment, plugin_resolver, &plugin_pools, &past_config, &file_path, &file_text);
                match result {
                    Ok((formatted_text, config)) => {
                        if formatted_text == file_text {
                            messenger.send_message(0, Vec::new())?; // no change
                        } else {
                            messenger.send_message(1, vec![ // change
                                formatted_text.into()
                            ])?;
                        }

                        past_config.replace(config);
                    },
                    Err(err) => {
                        messenger.send_message(2, vec![ // error
                            err.to_string().into()
                        ])?;
                    }
                }
            },
            _ => {
                environment.log_error(&format!("Unknown message kind: {}", message_kind));
            }
        }
    }

    fn format_text<'a, TEnvironment: Environment>(
        args: &CliArgs,
        cache: &Cache<TEnvironment>,
        environment: &TEnvironment,
        plugin_resolver: &PluginResolver<TEnvironment>,
        plugin_pools: &Arc<PluginPools<TEnvironment>>,
        past_config: &Option<ResolvedConfig>,
        file_path: &Path,
        file_text: &'a str,
    ) -> Result<(Cow<'a, str>, ResolvedConfig), ErrBox> {
        let config = resolve_config_from_args(&args, cache, environment)?;
        let has_config_changed = past_config.is_none() || *past_config.as_ref().unwrap() != config;
        if has_config_changed {
            plugin_pools.drop_plugins(); // clear the existing plugins
            let plugins = resolve_plugins(&config, environment, plugin_resolver)?;
            plugin_pools.set_plugins(plugins);
        }

        let formatted_text = format_with_plugin_pools(&file_path, &file_text, environment, &plugin_pools)?;
        Ok((formatted_text, config))
    }
}

fn clear_cache(environment: &impl Environment) -> Result<(), ErrBox> {
    let cache_dir = environment.get_cache_dir();
    environment.remove_dir_all(&cache_dir)?;
    environment.log(&format!("Deleted {}", cache_dir.display()));
    Ok(())
}

fn get_plugins_from_args<TEnvironment: Environment>(
    args: &CliArgs,
    cache: &Cache<TEnvironment>,
    environment: &TEnvironment,
    plugin_resolver: &PluginResolver<TEnvironment>,
) -> Result<Vec<Box<dyn Plugin>>, ErrBox> {
    match resolve_config_from_args(args, cache, environment) {
        Ok(config) => resolve_plugins(&config, environment, plugin_resolver),
        Err(_) => Ok(Vec::new()), // ignore
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
        output_plugin_config_diagnostics(plugin.name(), &initialized_plugin, &ErrorCountLogger::from_environment(environment))?;

        let text = initialized_plugin.get_resolved_config()?;
        let pretty_text = pretty_print_json_text(&text)?;
        environment.log(&format!("{}: {}", config_key, pretty_text));
    }

    Ok(())
}

fn init_config_file(environment: &impl Environment, config_arg: &Option<String>) -> Result<(), ErrBox> {
    let config_file_path = get_config_path(config_arg)?;
    return if !environment.path_exists(&config_file_path) {
        environment.write_file(&config_file_path, &configuration::get_init_config_file_text(environment)?)?;
        environment.log(&format!("\nCreated {}", config_file_path.display()));
        environment.log("\nIf you are working in a commercial environment please consider sponsoring dprint: https://dprint.dev/sponsor");
        Ok(())
    } else {
        err!("Configuration file '{}' already exists.", config_file_path.display())
    };

    fn get_config_path(config_arg: &Option<String>) -> Result<PathBuf, ErrBox> {
        return Ok(if let Some(config_arg) = config_arg.as_ref() {
            PathBuf::from(config_arg)
        } else {
            PathBuf::from("./dprint.json")
        });
    }
}

fn output_stdin_format<TEnvironment: Environment>(
    file_name: &Path,
    file_text: &str,
    environment: &TEnvironment,
    plugin_pools: Arc<PluginPools<TEnvironment>>,
) -> Result<(), ErrBox> {
    let formatted_text = format_with_plugin_pools(file_name, file_text, environment, &plugin_pools)?;
    environment.log_silent(&formatted_text);
    Ok(())
}

fn format_with_plugin_pools<'a, TEnvironment: Environment>(
    file_name: &Path,
    file_text: &'a str,
    environment: &TEnvironment,
    plugin_pools: &Arc<PluginPools<TEnvironment>>,
) -> Result<Cow<'a, str>, ErrBox> {
    if let Some(plugin_name) = plugin_pools.get_plugin_name_from_file_name(file_name) {
        let plugin_pool = plugin_pools.get_pool(&plugin_name).unwrap();
        let error_logger = ErrorCountLogger::from_environment(environment);
        match plugin_pool.take_or_create_checking_config_diagnostics(&error_logger)? {
            TakePluginResult::Success(mut initialized_plugin) => {
                let result = initialized_plugin.format_text(file_name, file_text, &HashMap::new());
                plugin_pool.release(initialized_plugin);
                Ok(Cow::Owned(result?)) // release plugin above, then propagate this error
            }
            TakePluginResult::HadDiagnostics => {
                err!("Had {} configuration errors.", error_logger.get_error_count())
            }
        }
    } else {
        Ok(Cow::Borrowed(file_text))
    }
}

fn check_files<TEnvironment: Environment>(
    file_paths_by_plugin: HashMap<String, Vec<PathBuf>>,
    environment: &TEnvironment,
    plugin_pools: Arc<PluginPools<TEnvironment>>,
    incremental_file: Option<Arc<IncrementalFile<TEnvironment>>>,
) -> Result<(), ErrBox> {
    let not_formatted_files_count = Arc::new(AtomicUsize::new(0));

    run_parallelized(file_paths_by_plugin, environment, plugin_pools, incremental_file, {
        let not_formatted_files_count = not_formatted_files_count.clone();
        move |file_path, file_text, formatted_text, _, _, environment| {
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
            Ok(())
        }
    })?;

    let not_formatted_files_count = not_formatted_files_count.load(Ordering::SeqCst);
    if not_formatted_files_count == 0 {
        Ok(())
    } else {
        let f = if not_formatted_files_count == 1 { "file" } else { "files" };
        err!("Found {} not formatted {}.", not_formatted_files_count.to_string().bold().to_string(), f)
    }
}

fn format_files<TEnvironment: Environment>(
    file_paths_by_plugin: HashMap<String, Vec<PathBuf>>,
    environment: &TEnvironment,
    plugin_pools: Arc<PluginPools<TEnvironment>>,
    incremental_file: Option<Arc<IncrementalFile<TEnvironment>>>,
) -> Result<(), ErrBox> {
    let formatted_files_count = Arc::new(AtomicUsize::new(0));
    let files_count: usize = file_paths_by_plugin.values().map(|x| x.len()).sum();

    run_parallelized(file_paths_by_plugin, environment, plugin_pools, incremental_file.clone(), {
        let formatted_files_count = formatted_files_count.clone();
        move |file_path, file_text, formatted_text, had_bom, _, environment| {
            if formatted_text != file_text {
                let new_text = if had_bom {
                    // add back the BOM
                    format!("{}{}", BOM_CHAR, formatted_text)
                } else {
                    formatted_text
                };

                formatted_files_count.fetch_add(1, Ordering::SeqCst);
                environment.write_file(&file_path, &new_text)?;
            }

            Ok(())
        }
    })?;

    let formatted_files_count = formatted_files_count.load(Ordering::SeqCst);
    if formatted_files_count > 0 {
        let suffix = if files_count == 1 { "file" } else { "files" };
        environment.log(&format!("Formatted {} {}.", formatted_files_count.to_string().bold().to_string(), suffix));
    }

    if let Some(incremental_file) = &incremental_file {
        incremental_file.write();
    }

    Ok(())
}

fn output_format_times<TEnvironment: Environment>(
    file_paths_by_plugin: HashMap<String, Vec<PathBuf>>,
    environment: &TEnvironment,
    plugin_pools: Arc<PluginPools<TEnvironment>>,
) -> Result<(), ErrBox> {
    let durations: Arc<Mutex<Vec<(PathBuf, u128)>>> = Arc::new(Mutex::new(Vec::new()));

    run_parallelized(file_paths_by_plugin, environment, plugin_pools, None, {
        let durations = durations.clone();
        move |file_path, _, _, _, start_instant, _| {
            let duration = start_instant.elapsed().as_millis();
            let mut durations = durations.lock();
            durations.push((file_path.to_owned(), duration));
            Ok(())
        }
    })?;

    let mut durations = durations.lock();
    durations.sort_by_key(|k| k.1);
    for (file_path, duration) in durations.iter() {
        environment.log(&format!("{}ms - {}", duration, file_path.display()));
    }

    Ok(())
}

fn run_parallelized<F, TEnvironment: Environment>(
    file_paths_by_plugin: HashMap<String, Vec<PathBuf>>,
    environment: &TEnvironment,
    plugin_pools: Arc<PluginPools<TEnvironment>>,
    incremental_file: Option<Arc<IncrementalFile<TEnvironment>>>,
    f: F,
) -> Result<(), ErrBox> where F: Fn(&Path, &str, String, bool, Instant, &TEnvironment) -> Result<(), ErrBox> + Send + 'static + Clone {
    let error_logger = ErrorCountLogger::from_environment(environment);

    do_batch_format(environment, &error_logger, &plugin_pools, file_paths_by_plugin, {
        let environment = environment.clone();
        let incremental_file = incremental_file.clone();
        let error_logger = error_logger.clone();
        move |plugin_pool, file_path, plugin| {
            let result = run_for_file_path(&environment, &incremental_file, plugin_pool, file_path, plugin, f.clone());
            if let Err(err) = result {
                error_logger.log_error(&format!("Error formatting {}. Message: {}", file_path.display(), err.to_string()));
            }
        }
    })?;

    let error_count = error_logger.get_error_count();
    return if error_count == 0 {
        Ok(())
    } else {
        err!("Had {0} error(s) formatting.", error_count)
    };

    #[inline]
    fn run_for_file_path<F, TEnvironment: Environment>(
        environment: &TEnvironment,
        incremental_file: &Option<Arc<IncrementalFile<TEnvironment>>>,
        plugin_pool: &InitializedPluginPool<TEnvironment>,
        file_path: &Path,
        initialized_plugin: &mut Box<dyn InitializedPlugin>,
        f: F
    ) -> Result<(), ErrBox> where F: Fn(&Path, &str, String, bool, Instant, &TEnvironment) -> Result<(), ErrBox> + Send + 'static + Clone {
        let file_text = FileText::new(environment.read_file(&file_path)?);

        if let Some(incremental_file) = incremental_file {
            if incremental_file.is_file_same(file_path, file_text.as_str()) {
                log_verbose!(environment, "No change: {}", file_path.display());
                return Ok(());
            }
        }

        let (start_instant, formatted_text) = {
            let start_instant = Instant::now();
            let format_text_result = plugin_pool.format_measuring_time(|| {
                initialized_plugin.format_text(file_path, file_text.as_str(), &HashMap::new())
            });
            log_verbose!(environment, "Formatted file: {} in {}ms", file_path.display(), start_instant.elapsed().as_millis());
            (start_instant, format_text_result?)
        };

        if let Some(incremental_file) = incremental_file {
            incremental_file.update_file(file_path, &formatted_text);
        }

        f(&file_path, file_text.as_str(), formatted_text, file_text.has_bom(), start_instant, &environment)?;

        Ok(())
    }
}

fn resolve_plugins_and_err_if_empty<TEnvironment: Environment>(
    config: &ResolvedConfig,
    environment: &TEnvironment,
    plugin_resolver: &PluginResolver<TEnvironment>,
) -> Result<Vec<Box<dyn Plugin>>, ErrBox> {
    let plugins = resolve_plugins(config, environment, plugin_resolver)?;
    if plugins.is_empty() {
        return err!("No formatting plugins found. Ensure at least one is specified in the 'plugins' array of the configuration file.");
    }
    Ok(plugins)
}

fn resolve_plugins<TEnvironment: Environment>(
    config: &ResolvedConfig,
    environment: &TEnvironment,
    plugin_resolver: &PluginResolver<TEnvironment>,
) -> Result<Vec<Box<dyn Plugin>>, ErrBox> {
    // resolve the plugins
    let plugins = plugin_resolver.resolve_plugins(config.plugins.clone())?;
    let mut config_map = config.config_map.clone();

    // resolve each plugin's configuration
    let mut plugins_with_config = Vec::new();
    for plugin in plugins.into_iter() {
        plugins_with_config.push((
            get_plugin_config_map(&plugin, &mut config_map)?,
            plugin
        ));
    }

    // now get global config
    let global_config = get_global_config(config_map, environment)?;

    // now set each plugin's config
    let mut plugins = Vec::new();
    for (plugin_config, plugin) in plugins_with_config {
        let mut plugin = plugin;
        plugin.set_config(plugin_config, global_config.clone());
        plugins.push(plugin);
    }

    return Ok(plugins);
}

fn get_and_resolve_file_paths(config: &ResolvedConfig, args: &CliArgs, environment: &impl Environment) -> Result<Vec<PathBuf>, ErrBox> {
    let (file_patterns, absolute_paths) = get_config_file_paths(config, args, environment)?;
    return resolve_file_paths(&file_patterns, &absolute_paths, config, environment);
}

fn get_config_file_paths(config: &ResolvedConfig, args: &CliArgs, environment: &impl Environment) -> Result<(Vec<String>, Vec<PathBuf>), ErrBox> {
    let cwd = environment.cwd()?;
    let mut file_patterns = get_all_file_patterns(config, args, &cwd.to_string_lossy());
    let absolute_paths = take_absolute_paths(&mut file_patterns, environment);

    return Ok((file_patterns, absolute_paths));
}

fn resolve_file_paths(file_patterns: &Vec<String>, absolute_paths: &Vec<PathBuf>, config: &ResolvedConfig, environment: &impl Environment) -> Result<Vec<PathBuf>, ErrBox> {
    let mut file_paths = environment.glob(&config.base_path, file_patterns)?;
    file_paths.extend(absolute_paths.clone());
    return Ok(file_paths);
}

fn get_all_file_patterns(config: &ResolvedConfig, args: &CliArgs, cwd: &str) -> Vec<String> {
    let mut include_file_patterns = get_include_file_patterns(config, args, cwd);
    let mut exclude_file_patterns = get_exclude_file_patterns(config, args, cwd);
    include_file_patterns.append(&mut exclude_file_patterns);
    return include_file_patterns;
}

fn get_include_file_patterns(config: &ResolvedConfig, args: &CliArgs, cwd: &str) -> Vec<String> {
    let mut file_patterns = Vec::new();

    file_patterns.extend(if args.file_patterns.is_empty() {
        config.includes.clone()
    } else {
        // resolve CLI patterns based on the current working directory
        to_absolute_globs(&args.file_patterns, cwd)
    });

    process_file_pattern_slashes(&mut file_patterns);
    return file_patterns;
}

fn get_exclude_file_patterns(config: &ResolvedConfig, args: &CliArgs, cwd: &str) -> Vec<String> {
    let mut file_patterns = Vec::new();

    file_patterns.extend(if args.exclude_file_patterns.is_empty() {
        config.excludes.clone()
    } else {
        // resolve CLI patterns based on the current working directory
        to_absolute_globs(&args.exclude_file_patterns, cwd)
    }.into_iter().map(|exclude| if exclude.starts_with("!") { exclude } else { format!("!{}", exclude) }));

    if !args.allow_node_modules {
        // glob walker will not search the children of a directory once it's ignored like this
        let node_modules_exclude = String::from("!**/node_modules");
        if !file_patterns.contains(&node_modules_exclude) {
            file_patterns.push(node_modules_exclude);
        }
    }
    process_file_pattern_slashes(&mut file_patterns);
    return file_patterns;
}

fn process_file_pattern_slashes(file_patterns: &mut Vec<String>) {
    for file_pattern in file_patterns.iter_mut() {
        // Convert all backslashes to forward slashes.
        // It is true that this means someone cannot specify patterns that
        // match files with backslashes in their name on Linux, however,
        // it is more desirable for this CLI to work the same way no matter
        // what operation system the user is on and for the CLI to match
        // backslashes as a path separator.
        *file_pattern = file_pattern.replace("\\", "/");

        // glob walker doesn't support having `./` at the front of paths, so just remove them when they appear
        if file_pattern.starts_with("./") {
            *file_pattern = String::from(&file_pattern[2..]);
        }
        if file_pattern.starts_with("!./") {
            *file_pattern = format!("!{}", &file_pattern[3..]);
        }
    }
}

fn to_absolute_globs(file_patterns: &Vec<String>, base_dir: &str) -> Vec<String> {
    file_patterns.iter().map(|p| to_absolute_glob(p, base_dir)).collect()
}

fn take_absolute_paths(file_patterns: &mut Vec<String>, environment: &impl Environment) -> Vec<PathBuf> {
    let len = file_patterns.len();
    let mut file_paths = Vec::new();
    for i in (0..len).rev() {
        if is_absolute_path(&file_patterns[i], environment) {
            file_paths.push(PathBuf::from(file_patterns.swap_remove(i))); // faster
        }
    }
    file_paths
}

fn is_absolute_path(file_pattern: &str, environment: &impl Environment) -> bool {
    return !has_glob_chars(file_pattern)
        && environment.is_absolute_path(file_pattern);

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

fn get_incremental_file<TEnvironment: Environment>(
    args: &CliArgs,
    config: &ResolvedConfig,
    cache: &Cache<TEnvironment>,
    plugin_pools: &PluginPools<TEnvironment>,
    environment: &TEnvironment,
) -> Option<Arc<IncrementalFile<TEnvironment>>> {
    if args.incremental || config.incremental {
        // the incremental file is stored in the cache with a key based on the root directory
        let base_path = match environment.canonicalize(&config.base_path) {
            Ok(base_path) => base_path,
            Err(err) => {
                environment.log_error(&format!("Could not canonicalize base path for incremental feature. {}", err));
                return None;
            }
        };
        let key = format!("incremental_cache:{}", base_path.to_string_lossy());
        let cache_item = if let Some(cache_item) = cache.get_cache_item(&key) {
            cache_item
        } else {
            let cache_item = cache.create_cache_item(CreateCacheItemOptions {
                key,
                extension: "incremental",
                bytes: None,
                meta_data: None,
            });
            match cache_item {
                Ok(cache_item) => cache_item,
                Err(err) => {
                    environment.log_error(&format!("Could not create cache item for incremental feature. {}", err));
                    return None;
                }
            }
        };
        let file_path = cache.resolve_cache_item_file_path(&cache_item);
        Some(Arc::new(IncrementalFile::new(file_path, plugin_pools.get_plugins_hash(), environment.clone(), base_path)))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use crossterm::style::Stylize;
    use dprint_core::types::ErrBox;
    use dprint_core::plugins::process::{StdIoReaderWriter, StdIoMessenger};
    use pretty_assertions::assert_eq;
    use std::path::{Path, PathBuf};
    use std::io::{Read, Write};

    use crate::environment::{Environment, TestEnvironment, TestEnvironmentBuilder};
    use crate::configuration::*;
    use crate::cli::TestStdInReader;
    use crate::test_helpers::{self, run_test_cli, run_test_cli_with_stdin};
    use crate::utils::get_difference;

    #[test]
    fn it_should_output_version_with_v() {
        let environment = TestEnvironment::new();
        run_test_cli(vec!["-v"], &environment).unwrap();
        let logged_messages = environment.take_logged_messages();
        assert_eq!(logged_messages, vec![format!("dprint {}", env!("CARGO_PKG_VERSION"))]);
    }

    #[test]
    fn it_should_output_version_with_no_plugins() {
        let environment = TestEnvironment::new();
        run_test_cli(vec!["--version"], &environment).unwrap();
        let logged_messages = environment.take_logged_messages();
        assert_eq!(logged_messages, vec![format!("dprint {}", env!("CARGO_PKG_VERSION"))]);
    }

    #[test]
    fn it_should_output_version_and_ignore_plugins() {
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin().build();
        run_test_cli(vec!["--version"], &environment).unwrap();
        let logged_messages = environment.take_logged_messages();
        assert_eq!(logged_messages, vec![format!("dprint {}", env!("CARGO_PKG_VERSION"))]);
    }

    #[test]
    fn it_should_output_help_with_no_plugins() {
        let environment = TestEnvironment::new();
        run_test_cli(vec!["--help"], &environment).unwrap();
        let logged_messages = environment.take_logged_messages();
        assert_eq!(logged_messages, vec![get_expected_help_text()]);
    }

    #[test]
    fn it_should_output_help_no_sub_commands() {
        let environment = TestEnvironment::new();
        run_test_cli(vec![], &environment).unwrap();
        let logged_messages = environment.take_logged_messages();
        assert_eq!(logged_messages, vec![get_expected_help_text()]);
    }

    #[test]
    fn it_should_output_help_with_plugins() {
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin().build();

        run_test_cli(vec!["--help"], &environment).unwrap();
        assert_eq!(environment.take_logged_messages(), vec![
            get_expected_help_text(),
            "\nPLUGINS HELP:",
            "    test-plugin         https://dprint.dev/plugins/test\n    test-process-plugin https://dprint.dev/plugins/test-process"
        ]);
    }

    #[test]
    fn it_should_output_resolved_config() {
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin().build();
        run_test_cli(vec!["output-resolved-config"], &environment).unwrap();
        assert_eq!(environment.take_logged_messages(), vec![
            "test-plugin: {\n  \"ending\": \"formatted\",\n  \"lineWidth\": 120\n}",
            "testProcessPlugin: {\n  \"ending\": \"formatted_process\",\n  \"lineWidth\": 120\n}",
        ]);
    }

    #[test]
    fn it_should_output_resolved_file_paths() {
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin()
            .write_file("/file.txt", "const t=4;")
            .write_file("/file2.txt", "const t=4;")
            .write_file("/file3.txt_ps", "const t=4;")
            .build();
        run_test_cli(vec!["output-file-paths", "**/*.*"], &environment).unwrap();
        let mut logged_messages = environment.take_logged_messages();
        logged_messages.sort();
        assert_eq!(logged_messages, vec!["/file.txt", "/file2.txt", "/file3.txt_ps"]);
    }

    #[test]
    fn it_should_not_output_file_paths_not_supported_by_plugins() {
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin()
            .write_file("/file.ts", "const t=4;")
            .write_file("/file2.ts", "const t=4;")
            .build();
        run_test_cli(vec!["output-file-paths", "**/*.*"], &environment).unwrap();
        assert_eq!(environment.take_logged_messages().len(), 0);
    }

    #[test]
    fn it_should_output_resolved_file_paths_when_using_backslashes() {
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin()
            .write_file("/file.txt", "const t=4;")
            .write_file("/file2.txt", "const t=4;")
            .write_file("/file3.txt_ps", "const t=4;")
            .build();
        run_test_cli(vec!["output-file-paths", "**\\*.*"], &environment).unwrap();
        let mut logged_messages = environment.take_logged_messages();
        logged_messages.sort();
        assert_eq!(logged_messages, vec!["/file.txt", "/file2.txt", "/file3.txt_ps"]);
    }

    #[test]
    fn it_should_output_format_times() {
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin()
            .write_file("/file.txt", "const t=4;")
            .write_file("/file2.txt", "const t=4;")
            .write_file("/file3.txt_ps", "const t=4;")
            .build();
        run_test_cli(vec!["output-format-times", "**/*.*"], &environment).unwrap();
        let logged_messages = environment.take_logged_messages();
        assert_eq!(logged_messages.len(), 3); // good enough
    }

    #[test]
    fn it_should_format_file() {
        let file_path1 = "/file.txt";
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
            .write_file(file_path1, "text")
            .build();
        run_test_cli(vec!["fmt", "/file.txt"], &environment).unwrap();
        assert_eq!(environment.take_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.take_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "text_formatted");
    }

    #[test]
    fn it_should_format_files() {
        let file_path1 = "/file.txt";
        let file_path2 = "/file.txt_ps";
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin()
            .write_file(&file_path1, "text")
            .write_file(&file_path2, "text2")
            .build();
        run_test_cli(vec!["fmt", "/file.*"], &environment).unwrap();
        assert_eq!(environment.take_logged_messages(), vec![get_plural_formatted_text(2)]);
        assert_eq!(environment.take_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "text_formatted");
        assert_eq!(environment.read_file(&file_path2).unwrap(), "text2_formatted_process");
    }

    #[test]
    fn it_should_format_plugin_explicitly_specified_files() {
        // this file name is mentioned in test-process-plugin's PluginInfo
        let file_path1 = "/test-process-plugin-exact-file";
        let environment = TestEnvironmentBuilder::with_initialized_remote_process_plugin()
            .write_file(&file_path1, "text")
            .build();
        run_test_cli(vec!["fmt", "*"], &environment).unwrap();
        assert_eq!(environment.take_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.take_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "text_formatted_process");
    }

    #[test]
    fn it_should_format_files_with_local_plugin() {
        let file_path = "/file.txt";
        let environment = TestEnvironmentBuilder::new()
            .add_local_wasm_plugin()
            .with_default_config(|c| {
                c.add_local_wasm_plugin();
            })
            .write_file(&file_path, "text")
            .initialize()
            .build();
        run_test_cli(vec!["fmt", "/file.txt"], &environment).unwrap();
        assert_eq!(environment.take_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.take_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path).unwrap(), "text_formatted");
    }

    #[test]
    fn it_should_handle_wasm_plugin_erroring() {
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
            .write_file("/file.txt", "should_error") // special text that makes the plugin error
            .build();
        let error_message = run_test_cli(vec!["fmt", "/file.txt"], &environment).err().unwrap();
        assert_eq!(environment.take_logged_messages().len(), 0);
        assert_eq!(environment.take_logged_errors(), vec![String::from("Error formatting /file.txt. Message: Did error.")]);
        assert_eq!(error_message.to_string(), "Had 1 error(s) formatting.");
    }

    #[test]
    fn it_should_handle_process_plugin_erroring() {
        let environment = TestEnvironmentBuilder::with_initialized_remote_process_plugin()
            .write_file("/file.txt_ps", "should_error") // special text that makes the plugin error
            .build();
        let error_message = run_test_cli(vec!["fmt", "/file.txt_ps"], &environment).err().unwrap();
        assert_eq!(environment.take_logged_messages().len(), 0);
        assert_eq!(environment.take_logged_errors(), vec![String::from("Error formatting /file.txt_ps. Message: Did error.")]);
        assert_eq!(error_message.to_string(), "Had 1 error(s) formatting.");
    }

    #[test]
    fn it_should_handle_wasm_plugin_panicking() {
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
            .write_file("/file1.txt", "should_panic") // special text to make it panic
            .write_file("/file2.txt", "test")
            .build();
        let error_message = run_test_cli(vec!["fmt", "**.txt"], &environment).err().unwrap();
        assert_eq!(environment.take_logged_messages().len(), 0);
        let logged_errors = environment.take_logged_errors();
        assert_eq!(logged_errors.len(), 1);
        assert_eq!(logged_errors[0].starts_with("Error formatting /file1.txt. Message: RuntimeError: unreachable"), true);
        assert_eq!(error_message.to_string(), "Had 1 error(s) formatting.");
        assert_eq!(environment.read_file("/file2.txt").unwrap(), "test_formatted");
    }

    #[test]
    fn it_should_format_calling_process_plugin_with_wasm_plugin_and_no_plugin_exists() {
        let file_path = "/file.txt";
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
            .write_file(&file_path, "plugin: format this text")
            .build();
        run_test_cli(vec!["fmt", "/file.txt"], &environment).unwrap();
        assert_eq!(environment.take_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.take_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path).unwrap(), "format this text");
    }

    #[test]
    fn it_should_format_calling_process_plugin_with_wasm_plugin_and_process_plugin_exists() {
        let file_path = "/file.txt";
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin()
            .write_file(&file_path, "plugin: format this text")
            .build();
        run_test_cli(vec!["fmt", "/file.txt"], &environment).unwrap();
        assert_eq!(environment.take_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.take_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path).unwrap(), "format this text_formatted_process");
    }

    #[test]
    fn it_should_format_calling_process_plugin_with_wasm_plugin_using_additional_plugin_specified_config() {
        let file_path1 = "/file1.txt";
        let file_path2 = "/file2.txt";
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin()
            .write_file(&file_path1, "plugin-config: format this text")
            .write_file(&file_path2, "plugin: format this text")
            .build();
        run_test_cli(vec!["fmt", "/*.txt"], &environment).unwrap();
        assert_eq!(environment.take_logged_messages(), vec![get_plural_formatted_text(2)]);
        assert_eq!(environment.take_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "format this text_custom_config");
        assert_eq!(environment.read_file(&file_path2).unwrap(), "format this text_formatted_process");
    }

    #[test]
    fn it_should_error_calling_process_plugin_with_wasm_plugin_and_process_plugin_errors() {
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin()
            .write_file("/file.txt", "plugin: should_error")
            .build();
        let error_message = run_test_cli(vec!["fmt", "/file.txt"], &environment).err().unwrap();
        assert_eq!(error_message.to_string(), "Had 1 error(s) formatting.");
        assert_eq!(environment.take_logged_errors(), vec![String::from("Error formatting /file.txt. Message: Did error.")]);
    }

    #[test]
    fn it_should_format_calling_other_plugin_with_process_plugin_and_no_plugin_exists() {
        let file_path = "/file.txt_ps";
        let environment = TestEnvironmentBuilder::with_initialized_remote_process_plugin()
            .write_file(&file_path, "plugin: format this text")
            .build();
        run_test_cli(vec!["fmt", "/file.txt_ps"], &environment).unwrap();
        assert_eq!(environment.take_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.take_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path).unwrap(), "format this text");
    }

    #[test]
    fn it_should_format_calling_wasm_plugin_with_process_plugin_and_wasm_plugin_exists() {
        let file_path = "/file.txt_ps";
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin()
            .write_file(&file_path, "plugin: format this text")
            .build();
        run_test_cli(vec!["fmt", "/file.txt_ps"], &environment).unwrap();
        assert_eq!(environment.take_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.take_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path).unwrap(), "format this text_formatted");
    }

    #[test]
    fn it_should_format_calling_wasm_plugin_with_process_plugin_using_additional_plugin_specified_config() {
        let file_path1 = "/file1.txt_ps";
        let file_path2 = "/file2.txt_ps";
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin()
            .write_file(&file_path1, "plugin-config: format this text")
            .write_file(&file_path2, "plugin: format this text")
            .build();
        run_test_cli(vec!["fmt", "*.txt_ps"], &environment).unwrap();
        assert_eq!(environment.take_logged_messages(), vec![get_plural_formatted_text(2)]);
        assert_eq!(environment.take_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "format this text_custom_config");
        assert_eq!(environment.read_file(&file_path2).unwrap(), "format this text_formatted");
    }

    #[test]
    fn it_should_error_calling_wasm_plugin_with_process_plugin_and_wasm_plugin_errors() {
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin()
            .write_file("/file.txt_ps", "plugin: should_error")
            .build();
        let error_message = run_test_cli(vec!["fmt", "/file.txt_ps"], &environment).err().unwrap();
        assert_eq!(error_message.to_string(), "Had 1 error(s) formatting.");
        assert_eq!(environment.take_logged_errors(), vec![String::from("Error formatting /file.txt_ps. Message: Did error.")]);
    }

    #[test]
    fn it_should_format_when_specifying_dot_slash_paths() {
        let file_path = "/file.txt";
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
            .write_file(&file_path, "text")
            .build();
        run_test_cli(vec!["fmt", "./file.txt"], &environment).unwrap();
        assert_eq!(environment.take_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.take_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path).unwrap(), "text_formatted");
    }

    #[test]
    fn it_should_exclude_a_specified_dot_slash_path() {
        let file_path = "/file.txt";
        let file_path2 = "/file2.txt";
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
            .write_file(&file_path, "text")
            .write_file(&file_path2, "text")
            .build();
        run_test_cli(vec!["fmt", "./**/*.txt", "--excludes", "./file2.txt"], &environment).unwrap();
        assert_eq!(environment.take_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.take_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path).unwrap(), "text_formatted");
        assert_eq!(environment.read_file(&file_path2).unwrap(), "text");
    }

    #[test]
    fn it_should_ignore_files_in_node_modules_by_default() {
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
            .write_file("/node_modules/file.txt", "")
            .write_file("/test/node_modules/file.txt", "")
            .write_file("/file.txt", "")
            .build();
        run_test_cli(vec!["fmt", "**/*.txt"], &environment).unwrap();
        assert_eq!(environment.take_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.take_logged_errors().len(), 0);
    }

    #[test]
    fn it_should_not_ignore_files_in_node_modules_when_allowed() {
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
            .write_file("/node_modules/file.txt", "const t=4;")
            .write_file("/test/node_modules/file.txt", "const t=4;")
            .build();
        run_test_cli(vec!["fmt", "--allow-node-modules", "**/*.txt"], &environment).unwrap();
        assert_eq!(environment.take_logged_messages(), vec![get_plural_formatted_text(2)]);
        assert_eq!(environment.take_logged_errors().len(), 0);
    }

    #[test]
    fn it_should_format_files_with_config() {
        let file_path1 = "/file1.txt";
        let file_path2 = "/file2.txt_ps";
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin()
            .with_local_config("/config.json", |c| {
                c.add_remote_wasm_plugin()
                    .add_remote_process_plugin()
                    .add_config_section("test-plugin", r#"{
                        "ending": "custom-formatted"
                    }"#)
                    .add_config_section("testProcessPlugin", r#"{
                        "ending": "custom-formatted2"
                    }"#);
            })
            .write_file(&file_path1, "text")
            .write_file(&file_path2, "text2")
            .build();

        run_test_cli(vec!["fmt", "--config", "/config.json", "/file1.txt", "/file2.txt_ps"], &environment).unwrap();

        assert_eq!(environment.take_logged_messages(), vec![get_plural_formatted_text(2)]);
        assert_eq!(environment.take_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "text_custom-formatted");
        assert_eq!(environment.read_file(&file_path2).unwrap(), "text2_custom-formatted2");
    }

    #[test]
    fn it_should_format_files_with_config_using_c() {
        let file_path1 = "/file1.txt";
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
            .write_file(file_path1, "text")
            .write_file("/config.json", r#"{
                "test-plugin": { "ending": "custom-formatted" },
                "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
            }"#)
            .build();

        run_test_cli(vec!["fmt", "-c", "/config.json", "/file1.txt"], &environment).unwrap();

        assert_eq!(environment.take_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.take_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "text_custom-formatted");
    }

    #[test]
    fn it_should_error_when_config_file_does_not_exist() {
        let environment = TestEnvironment::new();
        environment.write_file("/test.txt", "test").unwrap();

        let error_message = run_test_cli(vec!["fmt", "**/*.txt"], &environment).err().unwrap();

        assert_eq!(
            error_message.to_string(),
            concat!(
                "No config file found at ./dprint.json. Did you mean to create (dprint init) or specify one (--config <path>)?\n",
                "  Error: Could not find file at path ./dprint.json"
            )
        );
        assert_eq!(environment.take_logged_messages().len(), 0);
        assert_eq!(environment.take_logged_errors().len(), 0);
    }

    #[test]
    fn it_should_support_config_file_urls() {
        let file_path1 = "/file1.txt";
        let file_path2 = "/file2.txt";
        let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
            .with_remote_config("https://dprint.dev/test.json", |c| {
                c.add_remote_wasm_plugin()
                    .add_config_section("test-plugin", r#"{ "ending": "custom-formatted" }"#);
            })
            .write_file(&file_path1, "text")
            .write_file(&file_path2, "text2")
            .build();

        run_test_cli(vec!["fmt", "--config", "https://dprint.dev/test.json", "/file1.txt", "/file2.txt"], &environment).unwrap();

        assert_eq!(environment.take_logged_messages(), vec![get_plural_formatted_text(2)]);
        assert_eq!(environment.take_logged_errors(), vec!["Compiling https://plugins.dprint.dev/test-plugin.wasm"]);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "text_custom-formatted");
        assert_eq!(environment.read_file(&file_path2).unwrap(), "text2_custom-formatted");
    }

    #[test]
    fn it_should_error_on_wasm_plugin_config_diagnostic() {
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
            .with_default_config(|c| {
                c.add_config_section("test-plugin", r#"{ "non-existent": 25 }"#);
            })
            .write_file("/test.txt", "test")
            .build();

        let error_message = run_test_cli(vec!["fmt", "**/*.txt"], &environment).err().unwrap();

        assert_eq!(error_message.to_string(), "Had 1 error(s) formatting.");
        assert_eq!(environment.take_logged_messages().len(), 0);
        assert_eq!(environment.take_logged_errors(), vec![
            "[test-plugin]: Unknown property in configuration: non-existent",
            "[test-plugin]: Error initializing from configuration file. Had 1 diagnostic(s)."
        ]);
    }

    #[test]
    fn it_should_error_on_process_plugin_config_diagnostic() {
        let environment = TestEnvironmentBuilder::with_initialized_remote_process_plugin()
            .with_default_config(|c| {
                // Add this same plugin a few times in the configuration file for
                // some additional testing. It should handle it gracefully.
                c.add_remote_process_plugin();
                c.add_remote_process_plugin();

                c.add_config_section("testProcessPlugin", r#"{
                    "non-existent": 25
                }"#);
            })
            .write_file("/test.txt_ps", "test")
            .build();

        let error_message = run_test_cli(vec!["fmt", "**/*.txt_ps"], &environment).err().unwrap();

        assert_eq!(error_message.to_string(), "Had 1 error(s) formatting.");
        assert_eq!(environment.take_logged_messages().len(), 0);
        assert_eq!(environment.take_logged_errors(), vec![
            "[test-process-plugin]: Unknown property in configuration: non-existent",
            "[test-process-plugin]: Error initializing from configuration file. Had 1 diagnostic(s)."
        ]);
    }

    #[test]
    fn it_should_error_when_no_plugins_specified() {
        let environment = TestEnvironmentBuilder::new()
            .with_default_config(|c| {
                c.ensure_plugins_section();
            })
            .write_file("/test.txt", "test")
            .build();

        let error_message = run_test_cli(vec!["fmt", "**/*.txt"], &environment).err().unwrap();

        assert_eq!(error_message.to_string(), "No formatting plugins found. Ensure at least one is specified in the 'plugins' array of the configuration file.");
        assert_eq!(environment.take_logged_messages().len(), 0);
        assert_eq!(environment.take_logged_errors().len(), 0);
    }

    #[test]
    fn it_should_use_plugins_specified_in_cli_args() {
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
            .with_default_config(|c| {
                c.add_plugin("https://plugins.dprint.dev/other.wasm");
            })
            .write_file("/test.txt", "test")
            .build();

        run_test_cli(vec!["fmt", "**/*.txt", "--plugins", "https://plugins.dprint.dev/test-plugin.wasm"], &environment).unwrap();

        assert_eq!(environment.take_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.take_logged_errors().len(), 0);
    }

    #[test]
    fn it_should_allow_using_no_config_when_plugins_specified() {
        let environment = TestEnvironmentBuilder::new()
            .add_remote_wasm_plugin()
            .write_file("/test.txt", "test")
            .build();

        run_test_cli(vec!["fmt", "**/*.txt", "--plugins", "https://plugins.dprint.dev/test-plugin.wasm"], &environment).unwrap();

        assert_eq!(environment.take_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.take_logged_errors(), vec!["Compiling https://plugins.dprint.dev/test-plugin.wasm"]);
    }

    #[test]
    fn it_should_error_when_no_files_match_glob() {
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin().build();
        let error_message = run_test_cli(vec!["fmt", "**/*.txt"], &environment).err().unwrap();

        assert_eq!(
            error_message.to_string(),
            concat!(
                "No files found to format with the specified plugins. ",
                "You may want to try using `dprint output-file-paths` to see which files it's finding."
            )
        );
        assert_eq!(environment.take_logged_messages().len(), 0);
        assert_eq!(environment.take_logged_errors().len(), 0);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn it_should_format_absolute_paths_on_windows() {
        let file_path = "E:\\file1.txt";
        let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
            .with_local_config("D:\\test\\other\\dprint.json", |c| {
                c.add_includes("**/*.txt")
                    .add_remote_wasm_plugin();
            })
            .write_file(file_path, "text1")
            .set_cwd("D:\\test\\other\\")
            .initialize()
            .build();

        run_test_cli(vec!["fmt", "--", "E:\\file1.txt"], &environment).unwrap();

        assert_eq!(environment.take_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.take_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path).unwrap(), "text1_formatted");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn it_should_format_absolute_paths_on_linux() {
        let file_path = "/asdf/file1.txt";
        let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
            .with_local_config("/test/other/dprint.json", |c| {
                c.add_includes("**/*.txt")
                    .add_remote_wasm_plugin();
            })
            .write_file(&file_path, "text1")
            .set_cwd("/test/other/")
            .initialize()
            .build();

        run_test_cli(vec!["fmt", "--", "/asdf/file1.txt"], &environment).unwrap();

        assert_eq!(environment.take_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.take_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path).unwrap(), "text1_formatted");
    }

    #[test]
    fn it_should_format_files_with_config_includes() {
        let file_path1 = "/file1.txt";
        let file_path2 = "/file2.txt";
        let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
            .write_file(file_path1, "text1")
            .write_file(file_path2, "text2")
            .with_default_config(|c| {
                c.add_includes("**/*.txt")
                    .add_remote_wasm_plugin();
            })
            .initialize()
            .build();

        run_test_cli(vec!["fmt"], &environment).unwrap();

        assert_eq!(environment.take_logged_messages(), vec![get_plural_formatted_text(2)]);
        assert_eq!(environment.take_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "text1_formatted");
        assert_eq!(environment.read_file(&file_path2).unwrap(), "text2_formatted");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn it_should_format_files_with_config_includes_when_using_back_slashes() {
        let file_path1 = "/file1.txt";
        let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
            .with_default_config(|c| {
                c.add_includes("**\\\\*.txt") // escape for the json
                    .add_remote_wasm_plugin();
            })
            .write_file(file_path1, "text1")
            .initialize()
            .build();

        run_test_cli(vec!["fmt"], &environment).unwrap();

        assert_eq!(environment.take_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.take_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "text1_formatted");
    }

    #[test]
    fn it_should_override_config_includes_with_cli_includes() {
        let file_path1 = "/file1.txt";
        let file_path2 = "/file2.txt";
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
            .write_file(&file_path1, "text1")
            .write_file(&file_path2, "text2")
            .with_default_config(|c| {
                c.add_includes("**/*.txt")
                    .add_remote_wasm_plugin();
            })
            .build();

        run_test_cli(vec!["fmt", "/file1.txt"], &environment).unwrap();

        assert_eq!(environment.take_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.take_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "text1_formatted");
        assert_eq!(environment.read_file(&file_path2).unwrap(), "text2");
    }

    #[test]
    fn it_should_override_config_excludes_with_cli_excludes() {
        let file_path1 = "/file1.txt";
        let file_path2 = "/file2.txt";
        let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
            .write_file(&file_path1, "text1")
            .write_file(&file_path2, "text2")
            .with_default_config(|c| {
                c.add_includes("**/*.txt")
                    .add_excludes("/file1.txt")
                    .add_remote_wasm_plugin();
            })
            .initialize()
            .build();

        run_test_cli(vec!["fmt", "--excludes", "/file2.txt"], &environment).unwrap();

        assert_eq!(environment.take_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.take_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "text1_formatted");
        assert_eq!(environment.read_file(&file_path2).unwrap(), "text2");
    }

    #[test]
    fn it_should_support_clearing_config_excludes_with_cli_excludes_arg() {
        let file_path1 = "/file1.txt";
        let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
            .write_file(&file_path1, "text1")
            .with_default_config(|c| {
                c.add_includes("**/*.txt")
                    .add_excludes("/file1.txt")
                    .add_remote_wasm_plugin();
            })
            .initialize()
            .build();

        run_test_cli(vec!["fmt", "--excludes="], &environment).unwrap();

        assert_eq!(environment.take_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.take_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "text1_formatted");
    }

    #[test]
    fn it_should_format_explicitly_specified_file_even_if_excluded() {
        let file_path1 = "/file1.txt";
        let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
            .write_file(&file_path1, "text1")
            .with_default_config(|c| {
                c.add_includes("**/*.txt")
                    .add_excludes("/file1.txt")
                    .add_remote_wasm_plugin();
            })
            .initialize()
            .build();

        run_test_cli(vec!["fmt", "file1.txt"], &environment).unwrap();

        assert_eq!(environment.take_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.take_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "text1_formatted");
    }

    #[test]
    fn it_should_override_config_includes_and_excludes_with_cli() {
        let file_path1 = "/file1.txt";
        let file_path2 = "/file2.txt";
        let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
            .write_file(&file_path1, "text1")
            .write_file(&file_path2, "text2")
            .with_default_config(|c| {
                c.add_includes("/file2.txt")
                    .add_excludes("/file1.txt")
                    .add_remote_wasm_plugin();
            })
            .initialize()
            .build();
        run_test_cli(vec!["fmt", "/file1.txt", "--excludes", "/file2.txt"], &environment).unwrap();

        assert_eq!(environment.take_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.take_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "text1_formatted");
        assert_eq!(environment.read_file(&file_path2).unwrap(), "text2");
    }

    #[test]
    fn it_should_format_files_with_config_excludes() {
        let file_path1 = "/file1.txt";
        let file_path2 = "/file2.txt";
        let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
            .write_file(file_path1, "text1")
            .write_file(file_path2, "text2")
            .with_default_config(|c| {
                c.add_includes("**/*.txt")
                    .add_excludes("/file2.txt")
                    .add_remote_wasm_plugin();
            })
            .initialize()
            .build();

        run_test_cli(vec!["fmt"], &environment).unwrap();

        assert_eq!(environment.take_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.take_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "text1_formatted");
        assert_eq!(environment.read_file(&file_path2).unwrap(), "text2");
    }

    #[test]
    fn it_should_format_using_hidden_config_file_name() {
        let file_path = "/test/other/file.txt";
        let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
            .with_default_config(|c| {
                c.add_includes("**/*.txt")
                    .add_remote_wasm_plugin();
            })
            .set_cwd("/test/other/")
            .write_file(file_path, "text")
            .build();

        run_test_cli(vec!["fmt"], &environment).unwrap();
        assert_eq!(environment.take_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.read_file(&file_path).unwrap(), "text_formatted");
    }

    #[test]
    fn it_should_format_files_with_config_in_config_sub_dir_and_warn() {
        let file_path1 = "/file1.txt";
        let file_path2 = "/file2.txt";
        let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
            .write_file(file_path1, "text1")
            .write_file(file_path2, "text2")
            .with_local_config("./config/.dprintrc.json", |c| {
                c.add_includes("**/*.txt")
                    .add_remote_wasm_plugin();
            })
            .initialize()
            .build();

        run_test_cli(vec!["fmt"], &environment).unwrap();

        assert_eq!(environment.take_logged_messages(), vec![get_plural_formatted_text(2)]);
        assert_eq!(environment.take_logged_errors(), vec![
            "WARNING: Automatic resolution of the configuration file in the config sub directory will be deprecated soon. Please move the configuration file to the parent directory.",
            "WARNING: .dprintrc.json will be deprecated soon. Please rename it to dprint.json"
        ]);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "text1_formatted");
        assert_eq!(environment.read_file(&file_path2).unwrap(), "text2_formatted");
    }

    #[test]
    fn it_should_format_using_config_in_ancestor_directory() {
        let file_path = "/test/other/file.txt";
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
            .with_default_config(|c| {
                c.add_includes("**/*.txt");
            })
            .write_file(&file_path, "text")
            .build();
        environment.set_cwd("/test/other/");
        run_test_cli(vec!["fmt"], &environment).unwrap();
        assert_eq!(environment.take_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.take_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path).unwrap(), "text_formatted");
    }

    #[test]
    fn it_should_format_using_old_config_file_name_and_warn() {
        let file_path = "/test/other/file.txt";
        let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
            .with_local_config("/.dprintrc.json", |c| {
                c.add_remote_wasm_plugin()
                    .add_includes("**/*.txt");
            })
            .initialize()
            .set_cwd("/test/other/")
            .write_file(&file_path, "text")
            .build();
        run_test_cli(vec!["fmt"], &environment).unwrap();
        assert_eq!(environment.take_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.take_logged_errors(), vec![
            "WARNING: .dprintrc.json will be deprecated soon. Please rename it to dprint.json"
        ]);
        assert_eq!(environment.read_file(&file_path).unwrap(), "text_formatted");
    }

    #[test]
    fn it_should_format_using_config_in_ancestor_directory_config_folder_and_warn() {
        let file_path = "/test/other/file.txt";
        let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
            .with_local_config("./config/.dprintrc.json", |c| {
                c.add_includes("**/*.txt")
                    .add_remote_wasm_plugin();
            })
            .initialize()
            .set_cwd("/test/other/")
            .write_file(&file_path, "text")
            .build();
        run_test_cli(vec!["fmt"], &environment).unwrap();
        assert_eq!(environment.take_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.take_logged_errors(), vec![
            "WARNING: Automatic resolution of the configuration file in the config sub directory will be deprecated soon. Please move the configuration file to the parent directory.",
            "WARNING: .dprintrc.json will be deprecated soon. Please rename it to dprint.json"
        ]);
        assert_eq!(environment.read_file(&file_path).unwrap(), "text_formatted");
    }

    #[test]
    fn it_should_format_incrementally_when_specified_on_cli() {
        let file_path1 = "/file1.txt";
        let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
            .with_default_config(|c| {
                c.add_includes("**/*.txt")
                    .add_remote_wasm_plugin();
            })
            .write_file(&file_path1, "text1")
            .initialize()
            .build();

        run_test_cli(vec!["fmt", "--incremental"], &environment).unwrap();

        assert_eq!(environment.take_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.take_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "text1_formatted");

        environment.clear_logs();
        run_test_cli(vec!["fmt", "--incremental", "--verbose"], &environment).unwrap();
        assert_eq!(environment.take_logged_messages().iter().any(|msg| msg.contains("No change: /file1.txt")), true);

        // update the file and ensure it's formatted
        environment.write_file(&file_path1, "asdf").unwrap();
        environment.clear_logs();
        run_test_cli(vec!["fmt", "--incremental"], &environment).unwrap();
        assert_eq!(environment.take_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.take_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "asdf_formatted");

        // update the global config and ensure it's formatted
        environment.write_file("./dprint.json", r#"{
            "indentWidth": 2,
            "includes": ["**/*.txt"],
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#).unwrap();
        environment.clear_logs();
        run_test_cli(vec!["fmt", "--incremental"], &environment).unwrap();
        assert_eq!(environment.take_logged_messages().iter().any(|msg| msg.contains("No change: /file1.txt")), false);

        // update the plugin config and ensure it's formatted
        environment.write_file("./dprint.json", r#"{
            "indentWidth": 2,
            "test-plugin": {
                "ending": "custom-formatted",
                "line_width": 80
            },
            "includes": ["**/*.txt"],
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#).unwrap();
        environment.clear_logs();
        run_test_cli(vec!["fmt", "--incremental"], &environment).unwrap();
        assert_eq!(environment.take_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.take_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "asdf_formatted_custom-formatted");

        // Try this a few times. There was a bug where the config hashmap was being serialized causing
        // random order and the hash to be new each time.
        for _ in 1..4 {
            environment.clear_logs();
            run_test_cli(vec!["fmt", "--incremental"], &environment).unwrap();
            assert_eq!(environment.take_logged_messages().iter().any(|msg| msg.contains("No change: /file1.txt")), false);
        }

        // change the cwd and ensure it's not formatted again
        environment.clear_logs();
        environment.set_cwd("/test/other/");
        run_test_cli(vec!["fmt", "--incremental", "--verbose"], &environment).unwrap();
        assert_eq!(environment.take_logged_messages().iter().any(|msg| msg.contains("No change: /file1.txt")), true);
    }

    #[test]
    fn it_should_format_incrementally_when_specified_via_config() {
        let file_path1 = "/file1.txt";
        let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
            .with_default_config(|c| {
                c.add_remote_wasm_plugin()
                    .add_includes("**/*.txt")
                    .set_incremental(true);
            })
            .initialize()
            .write_file(&file_path1, "text1")
            .build();

        run_test_cli(vec!["fmt"], &environment).unwrap();

        assert_eq!(environment.take_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.take_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path1).unwrap(), "text1_formatted");

        environment.clear_logs();
        run_test_cli(vec!["fmt", "--verbose"], &environment).unwrap();
        assert_eq!(environment.take_logged_messages().iter().any(|msg| msg.contains("No change: /file1.txt")), true);
    }

    #[test]
    fn it_should_not_output_when_no_files_need_formatting() {
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
            .write_file("/file.txt", "text_formatted")
            .build();
        run_test_cli(vec!["fmt", "/file.txt"], &environment).unwrap();
        assert_eq!(environment.take_logged_messages().len(), 0);
        assert_eq!(environment.take_logged_errors().len(), 0);
    }

    #[test]
    fn it_should_not_output_when_no_files_need_formatting_for_check() {
        let file_path = "/file.txt";
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
            .write_file(&file_path, "text_formatted")
            .build();
        run_test_cli(vec!["check", "/file.txt"], &environment).unwrap();
        assert_eq!(environment.take_logged_messages().len(), 0);
        assert_eq!(environment.take_logged_errors().len(), 0);
    }

    #[test]
    fn it_should_output_when_a_file_need_formatting_for_check() {
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
            .write_file("/file.txt", "const t=4;")
            .build();
        let error_message = run_test_cli(vec!["check", "/file.txt"], &environment).err().unwrap();
        assert_eq!(error_message.to_string(), get_singular_check_text());
        assert_eq!(environment.take_logged_messages(), vec![
            format!(
                "{}\n{}\n--",
                format!("{} /file.txt:", "from".bold().red().to_string()),
                get_difference("const t=4;", "const t=4;_formatted").unwrap(),
            ),
        ]);
        assert_eq!(environment.take_logged_errors().len(), 0);
    }

    #[test]
    fn it_should_output_when_files_need_formatting_for_check() {
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
            .write_file("/file1.txt", "const t=4;")
            .write_file("/file2.txt", "const t=5;")
            .build();

        let error_message = run_test_cli(vec!["check", "/file1.txt", "/file2.txt"], &environment).err().unwrap();
        assert_eq!(error_message.to_string(), get_plural_check_text(2));
        let mut logged_messages = environment.take_logged_messages();
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
        assert_eq!(environment.take_logged_errors().len(), 0);
    }

    #[test]
    fn it_should_initialize() {
        let environment = TestEnvironment::new();
        environment.add_remote_file(crate::plugins::REMOTE_INFO_URL, r#"{
            "schemaVersion": 3,
            "pluginSystemSchemaVersion": 3,
            "latest": [{
                "name": "dprint-plugin-typescript",
                "version": "0.17.2",
                "url": "https://plugins.dprint.dev/typescript-0.17.2.wasm",
                "fileExtensions": ["ts"],
                "configKey": "typescript",
                "configExcludes": []
            }, {
                "name": "dprint-plugin-jsonc",
                "version": "0.2.3",
                "url": "https://plugins.dprint.dev/json-0.2.3.wasm",
                "fileExtensions": ["json"],
                "fileNames": [],
                "configKey": "json",
                "configExcludes": []
            }]
        }"#.as_bytes());
        let expected_text = get_init_config_file_text(&environment).unwrap();
        environment.clear_logs();
        run_test_cli(vec!["init"], &environment).unwrap();
        assert_eq!(environment.take_logged_errors(), vec![
            "Select plugins (use the spacebar to select/deselect and then press enter when finished):"
        ]);
        assert_eq!(environment.take_logged_messages(), vec![
            "\nCreated ./dprint.json",
            "\nIf you are working in a commercial environment please consider sponsoring dprint: https://dprint.dev/sponsor"
        ]);
        assert_eq!(environment.read_file("./dprint.json").unwrap(), expected_text);
    }

    #[test]
    fn it_should_initialize_with_specified_config_path() {
        let environment = TestEnvironment::new();
        environment.add_remote_file(crate::plugins::REMOTE_INFO_URL, r#"{
            "schemaVersion": 3,
            "pluginSystemSchemaVersion": 3,
            "latest": [{
                "name": "dprint-plugin-typescript",
                "version": "0.17.2",
                "url": "https://plugins.dprint.dev/typescript-0.17.2.wasm",
                "fileExtensions": ["json"],
                "configKey": "typescript",
                "configExcludes": []
            }]
        }"#.as_bytes());
        let expected_text = get_init_config_file_text(&environment).unwrap();
        environment.clear_logs();
        run_test_cli(vec!["init", "--config", "./test.config.json"], &environment).unwrap();
        assert_eq!(environment.take_logged_errors(), vec![
            "Select plugins (use the spacebar to select/deselect and then press enter when finished):"
        ]);
        assert_eq!(environment.take_logged_messages(), vec![
            "\nCreated ./test.config.json",
            "\nIf you are working in a commercial environment please consider sponsoring dprint: https://dprint.dev/sponsor"
        ]);
        assert_eq!(environment.read_file("./test.config.json").unwrap(), expected_text);
    }

    #[test]
    fn it_should_error_when_config_file_exists_on_initialize() {
        let environment = TestEnvironmentBuilder::new()
            .with_default_config(|c| {
                c.add_includes("**/*.txt");
            })
            .build();
        let error_message = run_test_cli(vec!["init"], &environment).err().unwrap();
        assert_eq!(error_message.to_string(), "Configuration file './dprint.json' already exists.");
    }

    #[test]
    fn it_should_clear_cache_directory() {
        let environment = TestEnvironment::new();
        run_test_cli(vec!["clear-cache"], &environment).unwrap();
        assert_eq!(environment.take_logged_messages(), vec!["Deleted /cache"]);
        assert_eq!(environment.is_dir_deleted("/cache"), true);
    }

    #[test]
    fn it_should_handle_bom() {
        let file_path = "/file.txt";
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin()
            .write_file(&file_path, "\u{FEFF}text")
            .build();
        run_test_cli(vec!["fmt", "/file.txt"], &environment).unwrap();
        assert_eq!(environment.take_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.take_logged_errors().len(), 0);
        assert_eq!(environment.read_file(&file_path).unwrap(), "\u{FEFF}text_formatted");
    }

    #[test]
    fn it_should_output_license_for_sub_command_with_no_plugins() {
        let environment = TestEnvironment::new();
        run_test_cli(vec!["license"], &environment).unwrap();
        assert_eq!(environment.take_logged_messages(), vec![
            "==== DPRINT CLI LICENSE ====",
            std::str::from_utf8(include_bytes!("../../LICENSE")).unwrap()
        ]);
    }

    #[test]
    fn it_should_output_license_for_sub_command_with_plugins() {
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin().build();
        run_test_cli(vec!["license"], &environment).unwrap();
        assert_eq!(environment.take_logged_messages(), vec![
            "==== DPRINT CLI LICENSE ====",
            std::str::from_utf8(include_bytes!("../../LICENSE")).unwrap(),
            "\n==== TEST-PLUGIN LICENSE ====",
            r#"The MIT License (MIT)

Copyright (c) 2020 David Sherret

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
"#,
            "\n==== TEST-PROCESS-PLUGIN LICENSE ====",
            "License text."
        ]);
    }

    #[test]
    fn it_should_output_editor_plugin_info() {
        // it should not output anything when downloading plugins
        let environment = TestEnvironmentBuilder::new()
            .add_remote_process_plugin()
            .add_remote_wasm_plugin()
            .with_default_config(|c| {
                c.add_remote_wasm_plugin()
                    .add_remote_process_plugin();
            })
            .build(); // build only, don't initialize
        run_test_cli(vec!["editor-info"], &environment).unwrap();
        assert_eq!(environment.take_logged_messages(), vec![
            r#"{"schemaVersion":3,"plugins":[{"name":"test-plugin","fileExtensions":["txt"],"fileNames":[]},{"name":"test-process-plugin","fileExtensions":["txt_ps"],"fileNames":["test-process-plugin-exact-file"]}]}"#
        ]);
    }

    struct EditorServiceCommunicator {
        messenger: StdIoMessenger<Box<dyn Read + Send>, Box<dyn Write + Send>>,
    }

    impl EditorServiceCommunicator {
        pub fn new(
            stdin: Box<dyn Write + Send>,
            stdout: Box<dyn Read + Send>,
        ) -> Self {
            let reader_writer = StdIoReaderWriter::new(stdout, stdin);
            let messenger = StdIoMessenger::new(reader_writer);
            EditorServiceCommunicator {
                messenger,
            }
        }

        pub fn check_file(&mut self, file_path: &Path) -> Result<bool, ErrBox> {
            self.messenger.send_message(1, vec![
                file_path.into(),
            ])?;
            let response_code = self.messenger.read_code()?;
            self.messenger.read_zero_part_message()?;
            Ok(response_code == 1)
        }

        pub fn format_text(&mut self, file_path: &Path, file_text: &str) -> Result<Option<String>, ErrBox> {
            self.messenger.send_message(2, vec![
                file_path.into(),
                file_text.into(),
            ])?;
            let response_code = self.messenger.read_code()?;
            match response_code {
                0 => {
                    self.messenger.read_zero_part_message()?;
                    Ok(None)
                },
                1 => {
                    Ok(Some(self.messenger.read_single_part_string_message()?))
                },
                2 => err!("{}", self.messenger.read_single_part_error_message()?),
                _ => err!("Unknown result: {}", response_code),
            }
        }

        pub fn exit(&mut self) {
            self.messenger.send_message(0, vec![]).unwrap();
        }
    }

    #[test]
    fn it_should_format_for_editor_service() {
        let txt_file_path = PathBuf::from("/file.txt");
        let ts_file_path = PathBuf::from("/file.ts");
        let other_ext_path = PathBuf::from("/file.asdf");
        let environment = TestEnvironmentBuilder::new()
            .add_remote_wasm_plugin()
            .add_remote_process_plugin()
            .with_default_config(|c| {
                c.add_remote_wasm_plugin()
                    .add_remote_process_plugin()
                    .add_includes("**/*.{txt,ts}");
            })
            .write_file(&txt_file_path, "")
            .write_file(&ts_file_path, "")
            .write_file(&other_ext_path, "")
            .initialize()
            .build();
        let stdin = environment.stdin_writer();
        let stdout = environment.stdout_reader();

        let result = std::thread::spawn({
            let environment = environment.clone();
            move || {
                let mut communicator = EditorServiceCommunicator::new(stdin, stdout);

                assert_eq!(communicator.check_file(&txt_file_path).unwrap(), true);
                assert_eq!(communicator.check_file(&PathBuf::from("/non-existent.txt")).unwrap(), false);
                assert_eq!(communicator.check_file(&other_ext_path).unwrap(), false);
                assert_eq!(communicator.check_file(&ts_file_path).unwrap(), true);

                assert_eq!(communicator.format_text(&txt_file_path, "testing").unwrap().unwrap(), "testing_formatted");
                assert_eq!(communicator.format_text(&txt_file_path, "testing_formatted").unwrap().is_none(), true); // it is already formatted
                assert_eq!(communicator.format_text(&other_ext_path, "testing").unwrap().is_none(), true); // can't format
                assert_eq!(communicator.format_text(&txt_file_path, "plugin: format this text").unwrap().unwrap(), "format this text_formatted_process");
                assert_eq!(communicator.format_text(&txt_file_path, "should_error").err().unwrap().to_string(), "Did error.");
                assert_eq!(communicator.format_text(&txt_file_path, "plugin: should_error").err().unwrap().to_string(), "Did error.");
                assert_eq!(communicator.format_text(&PathBuf::from("/file.txt_ps"), "testing").unwrap().unwrap(), "testing_formatted_process");

                // write a new file and make sure the service picks up the changes
                environment.write_file(&PathBuf::from("./dprint.json"), r#"{
                    "includes": ["**/*.txt"],
                    "test-plugin": {
                        "ending": "new_ending"
                    },
                    "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
                }"#).unwrap();

                assert_eq!(communicator.check_file(&ts_file_path).unwrap(), false); // shouldn't match anymore
                assert_eq!(communicator.check_file(&txt_file_path).unwrap(), true); // still ok
                assert_eq!(communicator.format_text(&txt_file_path, "testing").unwrap().unwrap(), "testing_new_ending");

                communicator.exit();
            }
        });

        // usually this would be the editor's process id, but this is ok for testing purposes
        let pid = std::process::id().to_string();
        run_test_cli(vec!["editor-service", "--parent-pid", &pid], &environment).unwrap();

        result.join().unwrap();
    }

    #[test]
    fn it_should_format_for_stdin_fmt_with_file_name() {
        // it should not output anything when downloading plugins
        let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
            .with_default_config(|c| {
                c.add_includes("/test/**.txt")
                    .add_remote_wasm_plugin();
            })
            .build();

        let test_std_in = TestStdInReader::new_with_text("text");
        run_test_cli_with_stdin(vec!["fmt", "--stdin", "file.txt"], &environment, test_std_in).unwrap();
        // should format even though it wasn't matched because an absolute path wasn't provided
        assert_eq!(environment.take_logged_messages(), vec!["text_formatted"]);
        assert_eq!(environment.take_logged_errors().len(), 0);
    }

    #[test]
    fn it_should_format_for_stdin_fmt_with_extension() {
        let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
            .with_default_config(|c| {
                c.add_includes("/test/**.txt")
                    .add_remote_wasm_plugin();
            })
            .build();

        let test_std_in = TestStdInReader::new_with_text("text");
        run_test_cli_with_stdin(vec!["fmt", "--stdin", "txt"], &environment, test_std_in).unwrap();
        // should format even though it wasn't matched because an absolute path wasn't provided
        assert_eq!(environment.take_logged_messages(), vec!["text_formatted"]);
        assert_eq!(environment.take_logged_errors().len(), 0);
    }

    #[test]
    fn it_should_stdin_fmt_calling_other_plugin() {
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin().build();
        let test_std_in = TestStdInReader::new_with_text("plugin: format this text");
        run_test_cli_with_stdin(vec!["fmt", "--stdin", "file.txt"], &environment, test_std_in).unwrap();
        assert_eq!(environment.take_logged_messages(), vec!["format this text_formatted_process"]);
    }

    #[test]
    fn it_should_handle_error_for_stdin_fmt() {
        // it should not output anything when downloading plugins
        let environment = TestEnvironmentBuilder::new()
            .add_remote_wasm_plugin()
            .with_default_config(|c| {
                c.add_remote_wasm_plugin();
            })
            .build(); // don't initialize
        let test_std_in = TestStdInReader::new_with_text("should_error");
        let error_message = run_test_cli_with_stdin(vec!["fmt", "--stdin", "file.txt"], &environment, test_std_in).err().unwrap();
        assert_eq!(error_message.to_string(), "Did error.");
    }

    #[test]
    fn it_should_format_for_stdin_with_absolute_paths() {
        // it should not output anything when downloading plugins
        let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
            .with_default_config(|c| {
                c.add_includes("/src/**.*")
                    .add_remote_wasm_plugin();
            })
            .write_file("/file.txt", "")
            .write_file("/src/file.txt", "")
            .build();
        // not matching file
        let test_std_in = TestStdInReader::new_with_text("text");
        run_test_cli_with_stdin(vec!["fmt", "--stdin", "/file.txt"], &environment, test_std_in.clone()).unwrap();
        assert_eq!(environment.take_logged_messages(), vec!["text"]);

        // make it matching on the cli
        run_test_cli_with_stdin(vec!["fmt", "--stdin", "/file.txt", "--", "**/*.txt"], &environment, test_std_in.clone()).unwrap();
        assert_eq!(environment.take_logged_messages(), vec!["text_formatted"]);

        // matching file
        run_test_cli_with_stdin(vec!["fmt", "--stdin", "/src/file.txt"], &environment, test_std_in).unwrap();
        assert_eq!(environment.take_logged_messages(), vec!["text_formatted"]);
    }

    #[test]
    fn it_should_format_stdin_resolving_config_file_from_provided_path_when_relative() {
        let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
            .with_default_config(|c| {
                c.add_includes("./**/*.txt")
                .add_remote_wasm_plugin();
            })
            .with_local_config("./sub-dir/dprint.json", |c| {
                c.add_includes("./**/*.txt")
                    .add_remote_wasm_plugin()
                    .add_config_section("test-plugin", r#"{ "ending": "new_ending" }"#);
            })
            .build();
        let test_std_in = TestStdInReader::new_with_text("text");
        run_test_cli_with_stdin(vec!["fmt", "--stdin", "sub-dir/file.txt"], &environment, test_std_in).unwrap();
        assert_eq!(environment.take_logged_messages(), vec!["text_new_ending"]);
        assert_eq!(environment.take_logged_errors().len(), 0);
    }

    #[test]
    fn it_should_format_stdin_resolving_config_file_from_provided_path_when_absolute() {
        let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
            .with_default_config(|c| {
                c.add_includes("./**/*.txt")
                .add_remote_wasm_plugin();
            })
            .with_local_config("/sub-dir/dprint.json", |c| {
                c.add_includes("./**/*.txt")
                    .add_remote_wasm_plugin()
                    .add_config_section("test-plugin", r#"{ "ending": "new_ending" }"#);
            })
            .write_file("/sub-dir/file.txt", "test")
            .initialize()
            .build();
        let test_std_in = TestStdInReader::new_with_text("text");
        run_test_cli_with_stdin(vec!["fmt", "--stdin", "/sub-dir/file.txt"], &environment, test_std_in).unwrap();
        assert_eq!(environment.take_logged_messages(), vec!["text_new_ending"]);
        assert_eq!(environment.take_logged_errors().len(), 0);
    }

    #[test]
    fn it_should_error_if_process_plugin_has_no_checksum_in_config() {
        let environment = TestEnvironmentBuilder::with_initialized_remote_process_plugin()
            .with_default_config(|c| {
                c.add_plugin("https://plugins.dprint.dev/test-process.exe-plugin");
            })
            .write_file("/test.txt_ps", "")
            .build();
        let error_message = run_test_cli(vec!["fmt", "*.*"], &environment).err().unwrap();

        assert_eq!(
            error_message.to_string(),
            concat!(
                "The plugin 'https://plugins.dprint.dev/test-process.exe-plugin' must have a checksum specified for security reasons ",
                "since it is not a Wasm plugin. You may specify one by writing \"https://plugins.dprint.dev/test-process.exe-plugin@checksum-goes-here\" ",
                "when providing the url in the configuration file. Check the plugin's release notes for what ",
                "the checksum is or calculate it yourself if you trust the source (it's SHA-256)."
            )
        );
    }

    #[test]
    fn it_should_error_if_process_plugin_has_wrong_checksum_in_config() {
        let environment = TestEnvironmentBuilder::with_remote_process_plugin()
            .with_default_config(|c| {
                c.add_remote_process_plugin_with_checksum("asdf");
            })
            .write_file("/test.txt_ps", "")
            .build();
        let actual_plugin_file_checksum = test_helpers::get_test_process_plugin_checksum(&environment);
        let error_message = run_test_cli(vec!["fmt", "*.*"], &environment).err().unwrap();

        assert_eq!(
            error_message.to_string(),
            format!(
                "Error resolving plugin https://plugins.dprint.dev/test-process.exe-plugin: The checksum {} did not match the expected checksum of asdf.",
                actual_plugin_file_checksum,
            )
        );
    }

    #[test]
    fn it_should_error_if_wasm_plugin_has_wrong_checksum_in_config() {
        let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
            .with_default_config(|c| {
                c.add_remote_wasm_plugin_with_checksum("asdf");
            })
            .write_file("/test.txt", "")
            .build();
        let actual_plugin_file_checksum = test_helpers::get_test_wasm_plugin_checksum();
        let error_message = run_test_cli(vec!["fmt", "*.*"], &environment).err().unwrap();

        assert_eq!(
            error_message.to_string(),
            format!(
                "Error resolving plugin https://plugins.dprint.dev/test-plugin.wasm: The checksum {} did not match the expected checksum of asdf.",
                actual_plugin_file_checksum,
            )
        );
    }

    #[test]
    fn it_should_not_error_if_wasm_plugin_has_correct_checksum_in_config() {
        let actual_plugin_file_checksum = test_helpers::get_test_wasm_plugin_checksum();
        let environment = TestEnvironmentBuilder::with_remote_wasm_plugin()
            .with_default_config(|c| {
                c.add_remote_wasm_plugin_with_checksum(&actual_plugin_file_checksum);
            })
            .write_file("/test.txt", "text")
            .build();
        run_test_cli(vec!["fmt", "*.*"], &environment).unwrap();

        assert_eq!(environment.read_file("/test.txt").unwrap(), "text_formatted");
        assert_eq!(environment.take_logged_messages(), vec![get_singular_formatted_text()]);
        assert_eq!(environment.take_logged_errors(), vec!["Compiling https://plugins.dprint.dev/test-plugin.wasm"]);
    }

    #[test]
    fn it_should_error_if_process_plugin_has_wrong_checksum_in_file_for_zip() {
        let environment = TestEnvironmentBuilder::with_remote_process_plugin()
            .write_process_plugin_file("asdf")
            .with_default_config(|c| {
                c.add_remote_process_plugin();
            })
            .write_file("/test.txt_ps", "")
            .build();
        let actual_plugin_zip_file_checksum = test_helpers::get_test_process_plugin_zip_checksum(&environment);
        let error_message = run_test_cli(vec!["fmt", "*.*"], &environment).err().unwrap();

        assert_eq!(
            error_message.to_string(),
            format!(
                "Error resolving plugin https://plugins.dprint.dev/test-process.exe-plugin: The checksum {} did not match the expected checksum of asdf.",
                actual_plugin_zip_file_checksum,
            )
        );
    }

    // todo: implement way of running these tests all on their own

    #[test]
    fn it_should_format_many_files() {
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin().build();
        for i in 0..100 {
            let file_path = format!("/file{}.txt", i);
            environment.write_file(&file_path, &format!("text{}", i)).unwrap();
        }
        for i in 0..100 {
            let file_path = format!("/file{}.txt_ps", i);
            environment.write_file(&file_path, &format!("text{}", i)).unwrap();
        }

        run_test_cli(vec!["fmt", "/*.*"], &environment).unwrap();
        assert_eq!(environment.take_logged_messages(), vec![get_plural_formatted_text(200)]);
        assert_eq!(environment.take_logged_errors().len(), 0);

        for i in 0..100 {
            let file_path = format!("/file{}.txt", i);
            assert_eq!(environment.read_file(&file_path).unwrap(), format!("text{}_formatted", i));
        }
        for i in 0..100 {
            let file_path = format!("/file{}.txt_ps", i);
            assert_eq!(environment.read_file(&file_path).unwrap(), format!("text{}_formatted_process", i));
        }
    }

    #[test]
    fn it_should_error_once_on_config_diagnostic_many_files() {
        // configuration diagnostic should only be shown by one thread
        let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_plugin().build();
        environment.write_file("./dprint.json", r#"{
            "test-plugin": { "non-existent": 25 },
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"]
        }"#).unwrap();

        for i in 0..100 {
            let file_path = format!("/file{}.txt", i);
            environment.write_file(&file_path, &format!("text{}", i)).unwrap();
        }

        let error_message = run_test_cli(vec!["fmt", "**/*.txt"], &environment).err().unwrap();

        assert_eq!(error_message.to_string(), "Had 1 error(s) formatting.");
        assert_eq!(environment.take_logged_messages().len(), 0);
        assert_eq!(environment.take_logged_errors(), vec![
            "[test-plugin]: Unknown property in configuration: non-existent",
            "[test-plugin]: Error initializing from configuration file. Had 1 diagnostic(s)."
        ]);
    }

    #[test]
    #[cfg(windows)]
    fn it_should_install_and_uninstall_on_windows() {
        let environment = TestEnvironment::new();
        environment.ensure_system_path("C:\\other").unwrap();
        run_test_cli(vec!["hidden", "windows-install", "C:\\test"], &environment).unwrap();
        assert_eq!(environment.get_system_path_dirs(), vec![PathBuf::from("C:\\other"), PathBuf::from("C:\\test")]);
        run_test_cli(vec!["hidden", "windows-uninstall", "C:\\test"], &environment).unwrap();
        assert_eq!(environment.get_system_path_dirs(), vec![PathBuf::from("C:\\other")]);
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
Copyright 2020-2021 by David Sherret

Auto-formats source code based on the specified plugins.

USAGE:
    dprint <SUBCOMMAND> [OPTIONS] [--] [file patterns]...

SUBCOMMANDS:
    init                      Initializes a configuration file in the current directory.
    fmt                       Formats the source files and writes the result to the file system.
    check                     Checks for any files that haven't been formatted.
    output-file-paths         Prints the resolved file paths for the plugins based on the args and configuration.
    output-resolved-config    Prints the resolved configuration for the plugins based on the args and configuration.
    output-format-times       Prints the amount of time it takes to format each file. Use this for debugging.
    clear-cache               Deletes the plugin cache directory.
    license                   Outputs the software license.

More details at `dprint help <SUBCOMMAND>`

OPTIONS:
    -c, --config <config>            Path or url to JSON configuration file. Defaults to dprint.json or .dprint.json in
                                     current or ancestor directory when not provided.
        --plugins <urls/files>...    List of urls or file paths of plugins to use. This overrides what is specified in
                                     the config file.
        --verbose                    Prints additional diagnostic information.
    -v, --version                    Prints the version.

ENVIRONMENT VARIABLES:
    DPRINT_CACHE_DIR    The directory to store the dprint cache. Note that
                        this directory may be periodically deleted by the CLI.

GETTING STARTED:
    1. Navigate to the root directory of a code repository.
    2. Run `dprint init` to create a dprint.json file in that directory.
    3. Modify configuration file if necessary.
    4. Run `dprint fmt` or `dprint check`.

EXAMPLES:
    Write formatted files to file system:

      dprint fmt

    Check for files that haven't been formatted:

      dprint check

    Specify path to config file other than the default:

      dprint fmt --config path/to/config/dprint.json

    Search for files using the specified file patterns:

      dprint fmt "**/*.{ts,tsx,js,jsx,json}""#)
    }
}
