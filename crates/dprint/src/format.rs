use anyhow::bail;
use anyhow::Result;
use dprint_core::configuration::ConfigKeyMap;
use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;

use crate::environment::Environment;
use crate::incremental::IncrementalFile;
use crate::paths::PluginNames;
use crate::plugins::GetPluginResult;
use crate::plugins::InitializedPlugin;
use crate::plugins::PluginWrapper;
use crate::plugins::PluginsCollection;
use crate::utils::ErrorCountLogger;
use crate::utils::FileText;

struct TaskWork<TEnvironment: Environment> {
  semaphore: Arc<Semaphore>,
  plugins: Vec<Arc<PluginWrapper<TEnvironment>>>,
  file_paths: Vec<PathBuf>,
}

struct StoredSemaphore {
  finished: bool,
  permits: usize,
  semaphore: Arc<Semaphore>,
}

pub async fn run_parallelized<F, TEnvironment: Environment>(
  file_paths_by_plugins: HashMap<PluginNames, Vec<PathBuf>>,
  environment: &TEnvironment,
  plugin_collection: Arc<PluginsCollection<TEnvironment>>,
  incremental_file: Option<Arc<IncrementalFile<TEnvironment>>>,
  f: F,
) -> Result<()>
where
  F: Fn(&Path, &str, String, bool, Instant, &TEnvironment) -> Result<()> + Send + Sync + 'static + Clone,
{
  let number_threads = std::thread::available_parallelism().map(|p| p.get()).unwrap_or(4);
  log_verbose!(environment, "Thread count: {}", number_threads);

  let error_logger = ErrorCountLogger::from_environment(environment);

  let mut file_paths_by_plugins = file_paths_by_plugins.into_iter().collect::<Vec<_>>();
  // favour giving semaphore permits to ones with more items at the start
  file_paths_by_plugins.sort_by_key(|(_, file_paths)| 0i32 - file_paths.len() as i32);
  let collection_count = file_paths_by_plugins.len();
  let mut semaphores = Vec::with_capacity(collection_count);
  let task_works = file_paths_by_plugins
    .into_iter()
    .enumerate()
    .map(|(i, (plugin_names, file_paths))| {
      let plugins = plugin_names
        .names()
        .map(|plugin_name| plugin_collection.get_plugin(plugin_name).unwrap())
        .collect();
      let additional_thread = i < number_threads % collection_count;
      let permits = number_threads / collection_count + if additional_thread { 1 } else { 0 };
      let semaphore = Arc::new(Semaphore::new(permits));
      semaphores.push(StoredSemaphore {
        finished: false,
        permits,
        semaphore: semaphore.clone(),
      });
      TaskWork {
        semaphore,
        plugins,
        file_paths,
      }
    })
    .collect::<Vec<_>>();

  let semaphores = Arc::new(tokio::sync::Mutex::new(semaphores));
  let handles = task_works.into_iter().enumerate().map(|(index, task_work)| {
    tokio::task::spawn({
      let error_logger = error_logger.clone();
      let environment = environment.clone();
      let incremental_file = incremental_file.clone();
      let f = f.clone();
      let semaphores = semaphores.clone();
      async move {
        // resolve the plugins
        let mut plugins = Vec::with_capacity(task_work.plugins.len());
        for plugin_wrapper in task_work.plugins {
          let result = match plugin_wrapper.get_or_create_checking_config_diagnostics(error_logger.clone()).await {
            Ok(result) => result,
            Err(err) => {
              error_logger.log_error(&format!("Error creating plugin {}. Message: {}", plugin_wrapper.name(), err));
              return;
            }
          };
          plugins.push(match result {
            GetPluginResult::HadDiagnostics => {
              return;
            }
            GetPluginResult::Success(plugin) => plugin,
          })
        }

        let plugins = Arc::new(plugins);
        let format_handles = task_work.file_paths.into_iter().map(|file_path| {
          let environment = environment.clone();
          let incremental_file = incremental_file.clone();
          let f = f.clone();
          let semaphore = task_work.semaphore.clone();
          let plugins = plugins.clone();
          let error_logger = error_logger.clone();
          tokio::task::spawn(async move {
            let _permit = semaphore.acquire().await.unwrap();
            let result = run_for_file_path(environment, incremental_file, plugins, file_path.clone(), f).await;
            if let Err(err) = result {
              error_logger.log_error(&format!("Error formatting {}. Message: {}", file_path.display(), err));
            }
          })
        });
        futures::future::join_all(format_handles).await;

        // release the permits to other semaphores so other tasks start doing more work
        let mut semaphores = semaphores.lock().await;
        semaphores[index].finished = true;
        let permits = semaphores[index].permits;
        let mut remaining_semaphores = semaphores.iter_mut().filter(|s| !s.finished).collect::<Vec<_>>();
        // favour giving permits to tasks with less permits... this should more ideally
        // give permits to batches that look like they will take the longest to complete
        remaining_semaphores.sort_by_key(|s| s.permits);
        let remaining_len = remaining_semaphores.len();
        for (i, semaphore) in remaining_semaphores.iter_mut().enumerate() {
          let additional_permit = i < permits % remaining_len;
          let new_permits = permits / remaining_len + if additional_permit { 1 } else { 0 };
          if new_permits > 0 {
            semaphore.permits += new_permits;
            semaphore.semaphore.add_permits(new_permits);
          }
        }
      }
    })
  });
  futures::future::join_all(handles).await;

  let error_count = error_logger.get_error_count();
  return if error_count == 0 {
    Ok(())
  } else {
    bail!("Had {0} error(s) formatting.", error_count)
  };

  #[inline]
  async fn run_for_file_path<F, TEnvironment: Environment>(
    environment: TEnvironment,
    incremental_file: Option<Arc<IncrementalFile<TEnvironment>>>,
    plugins: Arc<Vec<Arc<dyn InitializedPlugin>>>,
    file_path: PathBuf,
    f: F,
  ) -> Result<()>
  where
    F: Fn(&Path, &str, String, bool, Instant, &TEnvironment) -> Result<()> + Send + 'static + Clone,
  {
    let file_text = FileText::new(environment.read_file(&file_path)?);

    if let Some(incremental_file) = &incremental_file {
      if incremental_file.is_file_same(&file_path, file_text.as_str()) {
        log_verbose!(environment, "No change: {}", file_path.display());
        return Ok(());
      }
    }

    let (start_instant, formatted_text) = {
      let start_instant = Instant::now();
      let mut file_text = Cow::Borrowed(file_text.as_str());
      let plugins_len = plugins.len();
      for (i, plugin) in plugins.iter().enumerate() {
        let start_instant = Instant::now();
        let format_text_result = plugin
          .format_text(file_path.to_path_buf(), file_text.to_string(), None, ConfigKeyMap::new())
          .await;
        log_verbose!(
          environment,
          "Formatted file: {} in {}ms{}",
          file_path.display(),
          start_instant.elapsed().as_millis(),
          if plugins_len > 1 {
            format!(" (Plugin {}/{})", i + 1, plugins_len)
          } else {
            String::new()
          },
        );
        if let Some(text) = format_text_result? {
          file_text = Cow::Owned(text);
        }
      }
      (start_instant, file_text.into_owned())
    };

    if let Some(incremental_file) = incremental_file {
      incremental_file.update_file(&file_path, &formatted_text);
    }

    f(&file_path, file_text.as_str(), formatted_text, file_text.has_bom(), start_instant, &environment)?;

    Ok(())
  }
}
