use anyhow::bail;
use anyhow::Result;
use dprint_core::async_runtime::future;
use dprint_core::configuration::ConfigKeyMap;
use dprint_core::plugins::CriticalFormatError;
use dprint_core::plugins::NullCancellationToken;
use std::borrow::Cow;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::environment::Environment;
use crate::incremental::IncrementalFile;
use crate::resolution::GetPluginResult;
use crate::resolution::InitializedPluginWithConfig;
use crate::resolution::InitializedPluginWithConfigFormatRequest;
use crate::resolution::PluginWithConfig;
use crate::resolution::PluginsScope;
use crate::resolution::PluginsScopeAndPaths;
use crate::utils::ErrorCountLogger;
use crate::utils::FileText;
use crate::utils::Semaphore;

struct TaskWork {
  semaphore: Rc<Semaphore>,
  plugins: Vec<Rc<PluginWithConfig>>,
  file_paths: Vec<PathBuf>,
}

struct StoredSemaphore {
  finished: bool,
  permits: usize,
  semaphore: Rc<Semaphore>,
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub struct EnsureStableFormat(pub bool);

pub async fn run_parallelized<F, TEnvironment: Environment>(
  scope_and_paths: PluginsScopeAndPaths<TEnvironment>,
  environment: &TEnvironment,
  incremental_file: Option<Arc<IncrementalFile<TEnvironment>>>,
  ensure_stable_format: EnsureStableFormat,
  f: F,
) -> Result<()>
where
  F: Fn(PathBuf, FileText, String, Instant, TEnvironment) -> Result<()> + 'static + Clone + Send + Sync,
{
  if let Some(config) = &scope_and_paths.scope.config {
    log_verbose!(environment, "Running for config: {}", config.resolved_path.file_path.display());
  }

  let max_threads = environment.max_threads();
  let number_process_plugins = scope_and_paths.scope.process_plugin_count();
  let reduction_count = number_process_plugins + 1; // + 1 for each process plugin's possible runtime thread and this runtime's thread
  let number_threads = if max_threads > reduction_count { max_threads - reduction_count } else { 1 };
  log_verbose!(environment, "Max threads: {}\nThread count: {}", max_threads, number_threads);

  let error_logger = ErrorCountLogger::from_environment(environment);

  let scope = Rc::new(scope_and_paths.scope);
  let mut file_paths_by_plugins = scope_and_paths.file_paths_by_plugins.into_vec();
  // favour giving semaphore permits to ones with more items at the start
  file_paths_by_plugins.sort_by_key(|(_, file_paths)| 0i32 - file_paths.len() as i32);
  let collection_count = file_paths_by_plugins.len();
  let mut semaphores = Vec::with_capacity(collection_count);
  let mut task_works = Vec::with_capacity(collection_count);
  for (i, (plugin_names, file_paths)) in file_paths_by_plugins.into_iter().enumerate() {
    let plugins = plugin_names.names().map(|plugin_name| scope.get_plugin(plugin_name)).collect();
    let additional_thread = i < number_threads % collection_count;
    let permits = number_threads / collection_count + if additional_thread { 1 } else { 0 };
    let semaphore = Rc::new(Semaphore::new(permits));
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

  let semaphores = Rc::new(RefCell::new(semaphores));
  let handles = task_works.into_iter().enumerate().map(|(index, task_work)| {
    dprint_core::async_runtime::spawn({
      let error_logger = error_logger.clone();
      let environment = environment.clone();
      let incremental_file = incremental_file.clone();
      let f = f.clone();
      let semaphores = semaphores.clone();
      let scope = scope.clone();
      async move {
        let _semaphore_permits = SemaphorePermitReleaser { index, semaphores };
        // resolve the plugins
        let mut plugins = Vec::with_capacity(task_work.plugins.len());
        for plugin in task_work.plugins {
          let result = match plugin.get_or_create_checking_config_diagnostics(&environment).await {
            Ok(result) => result,
            Err(err) => {
              error_logger.log_error(&format!("Error creating plugin {}. Message: {}", plugin.name(), err));
              return;
            }
          };
          plugins.push(match result {
            GetPluginResult::HadDiagnostics(count) => {
              error_logger.add_error_count(count);
              return;
            }
            GetPluginResult::Success(plugin) => plugin,
          })
        }

        let plugins = Rc::new(plugins);
        let mut format_handles = Vec::with_capacity(task_work.file_paths.len());
        for file_path in task_work.file_paths.into_iter() {
          let permit = match task_work.semaphore.acquire().await {
            Ok(permit) => permit,
            Err(_) => return, // semaphore was closed, so stop working
          };
          let semaphore = task_work.semaphore.clone();
          let environment = environment.clone();
          let incremental_file = incremental_file.clone();
          let f = f.clone();
          let plugins = plugins.clone();
          let error_logger = error_logger.clone();
          let scope = scope.clone();
          format_handles.push(dprint_core::async_runtime::spawn(async move {
            let long_format_token = CancellationToken::new();
            dprint_core::async_runtime::spawn({
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
            let result = run_for_file_path(environment, incremental_file, scope, plugins, file_path.clone(), ensure_stable_format, f).await;
            long_format_token.cancel();
            if let Err(err) = result {
              if let Some(err) = err.downcast_ref::<CriticalFormatError>() {
                error_logger.log_error(&format!(
                  "Critical error formatting {}. Cannot continue. Message: {:#}",
                  file_path.display(),
                  err
                ));
                semaphore.close(); // stop formatting
              } else {
                error_logger.log_error(&format!("Error formatting {}. Message: {:#}", file_path.display(), err));
              }
            }
            // drop the semaphore permit when we're all done
            drop(permit);
          }));
        }
        future::join_all(format_handles).await;
      }
    })
  });
  future::join_all(handles).await;

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
    scope: Rc<PluginsScope<TEnvironment>>,
    plugins: Rc<Vec<InitializedPluginWithConfig>>,
    file_path: PathBuf,
    ensure_stable_format: EnsureStableFormat,
    f: F,
  ) -> Result<()>
  where
    F: Fn(PathBuf, FileText, String, Instant, TEnvironment) -> Result<()> + 'static + Clone + Send + Sync,
  {
    // it's a big perf improvement to do this work on a blocking thread
    let result = dprint_core::async_runtime::spawn_blocking(move || {
      let file_text = FileText::new(environment.read_file(&file_path)?);

      if let Some(incremental_file) = &incremental_file {
        if incremental_file.is_file_known_formatted(file_text.as_str()) {
          log_verbose!(environment, "No change: {}", file_path.display());
          return Ok::<_, anyhow::Error>(None);
        }
      }
      Ok(Some((file_path, file_text, environment)))
    })
    .await
    .unwrap()?;

    let Some((file_path, file_text, environment)) = result else {
      return Ok(());
    };

    let (start_instant, formatted_text) =
      run_single_pass_for_file_path(environment.clone(), scope.clone(), plugins.clone(), file_path.clone(), file_text.as_str()).await?;

    let formatted_text = if ensure_stable_format.0 && formatted_text != file_text.as_str() {
      get_stabilized_format_text(environment.clone(), scope, plugins, file_path.clone(), formatted_text).await?
    } else {
      formatted_text
    };

    dprint_core::async_runtime::spawn_blocking(move || f(file_path, file_text, formatted_text, start_instant, environment)).await??;

    Ok(())
  }

  async fn get_stabilized_format_text<TEnvironment: Environment>(
    environment: TEnvironment,
    scope: Rc<PluginsScope<TEnvironment>>,
    plugins: Rc<Vec<InitializedPluginWithConfig>>,
    file_path: PathBuf,
    mut formatted_text: String,
  ) -> Result<String> {
    log_verbose!(environment, "Ensuring stable format: {}", file_path.display());
    let mut count = 0;
    loop {
      match run_single_pass_for_file_path(environment.clone(), scope.clone(), plugins.clone(), file_path.clone(), &formatted_text).await {
        Ok((_, next_pass_text)) => {
          if next_pass_text == formatted_text {
            return Ok(formatted_text);
          } else {
            formatted_text = next_pass_text;
            log_verbose!(environment, "Ensuring stable format failed on try {}: {}", count + 1, file_path.display());
          }
        }
        Err(err) => {
          bail!(
            concat!(
              "Formatting succeeded initially, but failed when ensuring a stable format. ",
              "This is most likely a bug in the plugin where the text it produces is not syntatically correct. ",
              "Please report this as a bug to the plugin that formatted this file.\n\n{:#}"
            ),
            err,
          )
        }
      }
      count += 1;
      if count == 5 {
        bail!(
          concat!(
            "Formatting not stable. Bailed after {} tries. This indicates a bug in the ",
            "plugin where it formats the file differently each time."
          ),
          count
        );
      }
    }
  }

  async fn run_single_pass_for_file_path<TEnvironment: Environment>(
    environment: TEnvironment,
    scope: Rc<PluginsScope<TEnvironment>>,
    plugins: Rc<Vec<InitializedPluginWithConfig>>,
    file_path: PathBuf,
    file_text: &str,
  ) -> Result<(Instant, String)> {
    let start_instant = Instant::now();
    let mut file_text = Cow::Borrowed(file_text);
    let plugins_len = plugins.len();
    for (i, plugin) in plugins.iter().enumerate() {
      let start_instant = Instant::now();
      let format_text_result = plugin
        .format_text(InitializedPluginWithConfigFormatRequest {
          file_path: file_path.to_path_buf(),
          file_text: file_text.to_string(),
          range: None,
          override_config: ConfigKeyMap::new(),
          on_host_format: scope.create_host_format_callback(),
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
    Ok((start_instant, file_text.into_owned()))
  }
}

/// Ensures all semaphores are released on drop
/// so that other threads can do more work.
struct SemaphorePermitReleaser {
  index: usize,
  semaphores: Rc<RefCell<Vec<StoredSemaphore>>>,
}

impl Drop for SemaphorePermitReleaser {
  fn drop(&mut self) {
    // release the permits to other semaphores so other tasks start doing more work
    let mut semaphores = self.semaphores.borrow_mut();
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
