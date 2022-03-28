use anyhow::bail;
use anyhow::Result;
use dprint_core::configuration::ConfigKeyMap;
use dprint_core::plugins::CriticalFormatError;
use dprint_core::plugins::NullCancellationToken;
use parking_lot::Mutex;
use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use crate::environment::Environment;
use crate::incremental::IncrementalFile;
use crate::paths::PluginNames;
use crate::plugins::GetPluginResult;
use crate::plugins::InitializedPlugin;
use crate::plugins::InitializedPluginFormatRequest;
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
  plugins_collection: Arc<PluginsCollection<TEnvironment>>,
  incremental_file: Option<Arc<IncrementalFile<TEnvironment>>>,
  f: F,
) -> Result<()>
where
  F: Fn(&Path, &str, String, bool, Instant, &TEnvironment) -> Result<()> + Send + Sync + 'static + Clone,
{
  let number_cores = std::thread::available_parallelism().map(|p| p.get()).unwrap_or(4);
  let number_process_plugins = plugins_collection.process_plugin_count();
  let reduction_count = number_process_plugins + 1; // + 1 for each process plugin's possible runtime thread and this runtime's thread
  let number_threads = if number_cores > reduction_count { number_cores - reduction_count } else { 1 };
  log_verbose!(environment, "Core count: {}\nThread count: {}", number_cores, number_threads);

  let error_logger = ErrorCountLogger::from_environment(environment);

  let mut file_paths_by_plugins = file_paths_by_plugins.into_iter().collect::<Vec<_>>();
  // favour giving semaphore permits to ones with more items at the start
  file_paths_by_plugins.sort_by_key(|(_, file_paths)| 0i32 - file_paths.len() as i32);
  let collection_count = file_paths_by_plugins.len();
  let mut semaphores = Vec::with_capacity(collection_count);
  let mut task_works = Vec::with_capacity(collection_count);
  for (i, (plugin_names, file_paths)) in file_paths_by_plugins.into_iter().enumerate() {
    let plugins = plugin_names.names().map(|plugin_name| plugins_collection.get_plugin(plugin_name)).collect();
    let additional_thread = i < number_threads % collection_count;
    let permits = number_threads / collection_count + if additional_thread { 1 } else { 0 };
    let semaphore = Arc::new(Semaphore::new(permits));
    semaphores.push(StoredSemaphore {
      finished: false,
      permits,
      semaphore: semaphore.clone(),
    });
    task_works.push(TaskWork {
      semaphore,
      plugins,
      file_paths,
    });
  }

  let semaphores = Arc::new(Mutex::new(semaphores));
  let handles = task_works.into_iter().enumerate().map(|(index, task_work)| {
    tokio::task::spawn({
      let error_logger = error_logger.clone();
      let environment = environment.clone();
      let incremental_file = incremental_file.clone();
      let f = f.clone();
      let semaphores = semaphores.clone();
      async move {
        let _semaphore_permits = SemaphorePermitReleaser { index, semaphores };
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
        let mut format_handles = Vec::with_capacity(task_work.file_paths.len());
        for file_path in task_work.file_paths.into_iter() {
          let permit = match task_work.semaphore.clone().acquire_owned().await {
            Ok(permit) => permit,
            Err(_) => return, // semaphore was closed, so stop working
          };
          let semaphore = task_work.semaphore.clone();
          let environment = environment.clone();
          let incremental_file = incremental_file.clone();
          let f = f.clone();
          let plugins = plugins.clone();
          let error_logger = error_logger.clone();
          format_handles.push(tokio::task::spawn(async move {
            let long_format_token = CancellationToken::new();
            tokio::task::spawn({
              let long_format_token = long_format_token.clone();
              let environment = environment.clone();
              let file_path = file_path.clone();
              async move {
                tokio::select! {
                  _ = long_format_token.cancelled() => {
                    // exit
                  }
                  _ = tokio::time::sleep(Duration::from_secs(10)) => {
                    environment.log_stderr(&format!("WARNING: Formatting is slow for {}", file_path.display()));
                  }
                }
              }
            });
            let result = run_for_file_path(environment, incremental_file, plugins, file_path.clone(), f).await;
            long_format_token.cancel();
            if let Err(err) = result {
              if let Some(err) = err.downcast_ref::<CriticalFormatError>() {
                error_logger.log_error(&format!("Critical error formatting {}. Cannot continue. Message: {}", file_path.display(), err));
                semaphore.close(); // stop formatting
              } else {
                error_logger.log_error(&format!("Error formatting {}. Message: {}", file_path.display(), err));
              }
            }
            // drop the semaphore permit when we're all done
            drop(permit);
          }));
        }
        futures::future::join_all(format_handles).await;
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
          .format_text(InitializedPluginFormatRequest {
            file_path: file_path.to_path_buf(),
            file_text: file_text.to_string(),
            range: None,
            override_config: ConfigKeyMap::new(),
            token: Arc::new(NullCancellationToken),
          })
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
          file_text = Cow::Owned(text)
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

/// Ensures all semaphores are released on drop
/// so that other threads can do more work.
struct SemaphorePermitReleaser {
  index: usize,
  semaphores: Arc<Mutex<Vec<StoredSemaphore>>>,
}

impl Drop for SemaphorePermitReleaser {
  fn drop(&mut self) {
    // release the permits to other semaphores so other tasks start doing more work
    let mut semaphores = self.semaphores.lock();
    semaphores[self.index].finished = true;
    let permits = semaphores[self.index].permits;
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
}
