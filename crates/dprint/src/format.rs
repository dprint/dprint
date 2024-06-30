use anyhow::bail;
use anyhow::Result;
use dprint_core::async_runtime::future;
use dprint_core::configuration::ConfigKeyMap;
use dprint_core::plugins::CriticalFormatError;
use dprint_core::plugins::NullCancellationToken;
use std::borrow::Cow;
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
use crate::utils::Semaphore;

struct TaskWork {
  semaphore: Rc<Semaphore>,
  plugins: Vec<Rc<PluginWithConfig>>,
  file_paths: Vec<PathBuf>,
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
  F: Fn(PathBuf, Vec<u8>, Vec<u8>, Instant, TEnvironment) -> Result<()> + 'static + Clone + Send + Sync,
{
  if let Some(config) = &scope_and_paths.scope.config {
    log_debug!(environment, "Running for config: {}", config.resolved_path.file_path.display());
  }

  let max_threads = environment.max_threads();
  let number_process_plugins = scope_and_paths.scope.process_plugin_count();
  let reduction_count = number_process_plugins + 1; // + 1 for each process plugin's possible runtime thread and this runtime's thread
  let number_threads = if max_threads > reduction_count { max_threads - reduction_count } else { 1 };
  log_debug!(environment, "Max threads: {}\nThread count: {}", max_threads, number_threads,);

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
    semaphores.push(semaphore.clone());
    task_works.push(TaskWork {
      semaphore,
      plugins,
      file_paths,
    });
  }

  let semaphores = Rc::new(semaphores);
  let cpu_task_token = CancellationToken::new();

  dprint_core::async_runtime::spawn({
    let semaphores = semaphores.clone();
    let environment = environment.clone();
    let cpu_task_token = cpu_task_token.clone();
    async move { run_cpu_throttling_task(&environment, number_threads, &semaphores, cpu_task_token).await }
  });

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
                    log_warn!(environment, "WARNING: Formatting is slow for {}", file_path.display());
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

  cpu_task_token.cancel();

  let error_count = error_logger.get_error_count();
  return if error_count == 0 {
    Ok(())
  } else {
    bail!("Had {} error{} formatting.", error_count, if error_count == 1 { "" } else { "s" })
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
    F: Fn(PathBuf, Vec<u8>, Vec<u8>, Instant, TEnvironment) -> Result<()> + 'static + Clone + Send + Sync,
  {
    // it's a big perf improvement to do this work on a blocking thread
    let result = dprint_core::async_runtime::spawn_blocking(move || {
      let file_text = environment.read_file_bytes(&file_path)?;

      if let Some(incremental_file) = &incremental_file {
        if incremental_file.is_file_known_formatted(&file_text) {
          log_debug!(environment, "No change: {}", file_path.display());
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
      run_single_pass_for_file_path(environment.clone(), scope.clone(), plugins.clone(), file_path.clone(), &file_text).await?;

    let formatted_text = if ensure_stable_format.0 && formatted_text != file_text {
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
    mut formatted_text: Vec<u8>,
  ) -> Result<Vec<u8>> {
    log_debug!(environment, "Ensuring stable format: {}", file_path.display());
    let mut count = 0;
    loop {
      match run_single_pass_for_file_path(environment.clone(), scope.clone(), plugins.clone(), file_path.clone(), &formatted_text).await {
        Ok((_, next_pass_text)) => {
          if next_pass_text == formatted_text {
            return Ok(formatted_text);
          } else {
            formatted_text = next_pass_text;
            log_debug!(environment, "Ensuring stable format failed on try {}: {}", count + 1, file_path.display());
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
    file_text: &[u8],
  ) -> Result<(Instant, Vec<u8>)> {
    let start_instant = Instant::now();
    let original_text = file_text;
    let mut file_text = Cow::Borrowed(file_text);
    let plugins_len = plugins.len();
    for (i, plugin) in plugins.iter().enumerate() {
      let start_instant = Instant::now();
      let format_text_result = plugin
        .format_text(InitializedPluginWithConfigFormatRequest {
          file_path: file_path.to_path_buf(),
          file_bytes: file_text.to_vec(),
          range: None,
          override_config: ConfigKeyMap::new(),
          on_host_format: scope.create_host_format_callback(),
          token: Arc::new(NullCancellationToken),
        })
        .await;
      log_debug!(
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

    // some heuristic to stop plugins accidentally formatting a file to empty
    const MIN_CHARS_TO_EMPTY: usize = 300;
    if file_text.len() < 100 && original_text.len() > MIN_CHARS_TO_EMPTY {
      let original_text = String::from_utf8_lossy(original_text);
      let new_text = String::from_utf8_lossy(&file_text);
      if original_text.trim().len() > MIN_CHARS_TO_EMPTY && new_text.trim().is_empty() {
        bail!(
          concat!(
            "The original file text was greater than {} characters, but the formatted text was empty. ",
            "Most likely this is a bug in the plugin and the dprint CLI has prevented the plugin from ",
            "formatting the file to an empty file. Please report this scenario.",
          ),
          MIN_CHARS_TO_EMPTY
        )
      }
    }

    Ok((start_instant, file_text.into_owned()))
  }
}

fn target_cpu_decrease_bound(number_threads: usize) -> u8 {
  if number_threads < 3 {
    100 // never decrease
  } else if number_threads >= 50 {
    97
  } else {
    std::cmp::max((100f64 - 100f64 / (number_threads as f64)) as u8, 50)
  }
}

fn target_cpu_increase_bound(number_threads: usize) -> u8 {
  if number_threads < 3 {
    0 // never increase
  } else if number_threads >= 50 {
    95
  } else {
    let target_cpu = target_cpu_decrease_bound(number_threads);
    let ratio = number_threads as f64 / 60f64;
    let target_cpu = target_cpu - std::cmp::min((5f64 * (1f64 - ratio)) as u8, target_cpu);
    target_cpu - std::cmp::min(target_cpu, (100f64 / number_threads as f64) as u8)
  }
}

async fn run_cpu_throttling_task(environment: &impl Environment, number_threads: usize, semaphores: &[Rc<Semaphore>], cpu_task_token: CancellationToken) {
  if environment.is_ci() {
    // don't bother doing this on the CI as we should be the only thing running
    return;
  }

  // It's ok to go full out for a few seconds on the person's machine
  // when they initially start formatting, but as they take a few seconds
  // to switch to do something else, we should then start throttling the CPU
  tokio::select! {
    _ = cpu_task_token.cancelled() => {
      return; // exit
    }
    _ = tokio::time::sleep(Duration::from_secs(5)) => {
    }
  }

  let mut throttled_times = 0;
  let decrease_bound = target_cpu_decrease_bound(number_threads);
  let increase_bound = target_cpu_increase_bound(number_threads);
  let mut last_cpu_usage = 0;

  // now check the CPU usage every few seconds and throttle
  // the amount of work being done so that we don't completely
  // takeover someone's computer
  loop {
    let cpu_usage = environment.cpu_usage().await;
    log_debug!(environment, "CPU usage: {}%", cpu_usage);
    if cpu_usage > decrease_bound {
      if throttle_cpu(semaphores) {
        log_debug!(environment, "High CPU. Reducing parallelism.");
        throttled_times += 1;
      }
    } else if throttled_times > 0 && last_cpu_usage < increase_bound && cpu_usage < increase_bound {
      // Whatever was running in the background might
      // not be using as much CPU at this point, so increase
      // the permits
      add_permits(semaphores, 1);
      throttled_times -= 1;
      log_debug!(environment, "Low CPU. Increasing parallelism.");
    }
    last_cpu_usage = cpu_usage;

    // wait a couple seconds before re-checking cpu usage
    tokio::select! {
      _ = cpu_task_token.cancelled() => {
        return; // exit
      }
      _ = tokio::time::sleep(Duration::from_secs(2)) => {
      }
    }
  }
}

fn throttle_cpu(semaphores: &[Rc<Semaphore>]) -> bool {
  let mut best_match: Option<&Rc<Semaphore>> = None;
  let mut total_max_permits = 0;
  for semaphore in semaphores.iter() {
    if semaphore.closed() || semaphore.max_permits() == 0 {
      continue;
    }
    if semaphore.acquired_permits() > semaphore.max_permits() {
      // The previous adjustment hasn't yet been applied. Wait for that
      // to complete so we don't over scale down.
      best_match = None;
      break;
    }
    total_max_permits += semaphore.max_permits();

    match &best_match {
      Some(current_best_match) => {
        if current_best_match.max_permits() < semaphore.max_permits() {
          best_match = Some(semaphore);
        }
      }
      None => {
        best_match = Some(semaphore);
      }
    }
  }

  // always ensure there will be at least 1 permit running
  if total_max_permits <= 1 {
    return false;
  }

  match best_match {
    Some(best_match) => {
      best_match.remove_permits(1);
      true
    }
    None => false,
  }
}

/// Ensures all semaphores are released on drop
/// so that other threads can do more work.
struct SemaphorePermitReleaser {
  index: usize,
  semaphores: Rc<Vec<Rc<Semaphore>>>,
}

impl Drop for SemaphorePermitReleaser {
  fn drop(&mut self) {
    // release the permits to other semaphores so other tasks start doing more work
    self.semaphores[self.index].close();
    let amount = self.semaphores[self.index].max_permits();
    add_permits(&self.semaphores, amount)
  }
}

fn add_permits(semaphores: &[Rc<Semaphore>], amount: usize) {
  let mut remaining_semaphores = semaphores.iter().filter(|s| !s.closed()).collect::<Vec<_>>();
  // favour giving permits to tasks with less permits... this should more ideally
  // give permits to batches that look like they will take the longest to complete
  remaining_semaphores.sort_by_key(|s| s.max_permits());
  let remaining_len = remaining_semaphores.len();
  for (i, semaphore) in remaining_semaphores.iter_mut().enumerate() {
    let additional_permit = i < amount % remaining_len;
    let new_permits = amount / remaining_len + if additional_permit { 1 } else { 0 };
    if new_permits > 0 {
      semaphore.add_permits(new_permits);
    }
  }
}

#[cfg(test)]
mod test {
  use std::rc::Rc;

  use super::*;
  use crate::utils::Semaphore;

  #[test]
  fn target_cpu_calc() {
    run_test(0, 0..100);
    run_test(1, 0..100);
    run_test(2, 0..100);
    run_test(3, 29..66);
    run_test(4, 46..75);
    run_test(5, 56..80);
    run_test(8, 71..87);
    run_test(10, 76..90);
    run_test(20, 87..95);
    run_test(30, 91..96);
    run_test(40, 94..97);
    run_test(49, 95..97);
    run_test(50, 95..97);
    run_test(100, 95..97);
    run_test(200, 95..97);

    #[track_caller]
    fn run_test(input: usize, bound: std::ops::Range<u8>) {
      let increase_bound = target_cpu_increase_bound(input);
      let decrease_bound = target_cpu_decrease_bound(input);
      assert_eq!(increase_bound..decrease_bound, bound);
    }
  }

  #[tokio::test]
  async fn test_throttle_cpu() {
    let semaphore1 = Rc::new(Semaphore::new(1));
    let semaphore2 = Rc::new(Semaphore::new(2));
    let permit1 = semaphore1.acquire().await;
    let permit2 = semaphore2.acquire().await;
    let permit3 = semaphore2.acquire().await;
    let semaphores = vec![semaphore1, semaphore2];
    assert!(throttle_cpu(&semaphores));
    assert_eq!(semaphores[1].max_permits(), 1);
    // still a pending removal
    assert!(!throttle_cpu(&semaphores));
    drop(permit2);
    assert!(throttle_cpu(&semaphores));
    assert_eq!(semaphores[0].max_permits(), 0);
    assert_eq!(semaphores[1].max_permits(), 1);
    // still a pending removal
    assert!(!throttle_cpu(&semaphores));
    drop(permit3);
    // only one permit remaining
    assert!(!throttle_cpu(&semaphores));
    drop(permit1);
    assert!(!throttle_cpu(&semaphores));
  }
}
