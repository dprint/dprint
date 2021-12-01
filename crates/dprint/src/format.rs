use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use dprint_cli_core::types::ErrBox;

use crate::environment::Environment;
use crate::incremental::IncrementalFile;
use crate::plugins::do_batch_format;
use crate::plugins::InitializedPlugin;
use crate::plugins::InitializedPluginPool;
use crate::plugins::PluginPools;
use crate::plugins::TakePluginResult;
use crate::utils::ErrorCountLogger;
use crate::utils::FileText;

pub fn format_with_plugin_pools<'a, TEnvironment: Environment>(
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

pub fn run_parallelized<F, TEnvironment: Environment>(
  file_paths_by_plugin: HashMap<String, Vec<PathBuf>>,
  environment: &TEnvironment,
  plugin_pools: Arc<PluginPools<TEnvironment>>,
  incremental_file: Option<Arc<IncrementalFile<TEnvironment>>>,
  f: F,
) -> Result<(), ErrBox>
where
  F: Fn(&Path, &str, String, bool, Instant, &TEnvironment) -> Result<(), ErrBox> + Send + 'static + Clone,
{
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
    f: F,
  ) -> Result<(), ErrBox>
  where
    F: Fn(&Path, &str, String, bool, Instant, &TEnvironment) -> Result<(), ErrBox> + Send + 'static + Clone,
  {
    let file_text = FileText::new(environment.read_file(&file_path)?);

    if let Some(incremental_file) = incremental_file {
      if incremental_file.is_file_same(file_path, file_text.as_str()) {
        log_verbose!(environment, "No change: {}", file_path.display());
        return Ok(());
      }
    }

    let (start_instant, formatted_text) = {
      let start_instant = Instant::now();
      let format_text_result = plugin_pool.format_measuring_time(|| initialized_plugin.format_text(file_path, file_text.as_str(), &HashMap::new()));
      log_verbose!(
        environment,
        "Formatted file: {} in {}ms",
        file_path.display(),
        start_instant.elapsed().as_millis()
      );
      (start_instant, format_text_result?)
    };

    if let Some(incremental_file) = incremental_file {
      incremental_file.update_file(file_path, &formatted_text);
    }

    f(&file_path, file_text.as_str(), formatted_text, file_text.has_bom(), start_instant, &environment)?;

    Ok(())
  }
}
