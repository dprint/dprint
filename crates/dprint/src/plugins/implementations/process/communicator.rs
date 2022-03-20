use crate::environment::Environment;
use crate::plugins::InitializedPluginFormatRequest;
use crate::plugins::PluginsCollection;
use anyhow::Result;
use dprint_core::configuration::ConfigKeyMap;
use dprint_core::configuration::ConfigurationDiagnostic;
use dprint_core::configuration::GlobalConfiguration;
use dprint_core::plugins::process::ProcessPluginCommunicator;
use dprint_core::plugins::FormatResult;
use std::path::PathBuf;
use std::sync::Arc;

// We only need to support having one configuration set at a time
// so hardcode this.
const CONFIG_ID: u32 = 1;

struct ProcessRestartInfo<TEnvironment: Environment> {
  environment: TEnvironment,
  plugin_name: String,
  executable_file_path: PathBuf,
  config: (GlobalConfiguration, ConfigKeyMap),
  plugin_collection: Arc<PluginsCollection<TEnvironment>>,
}

pub struct InitializedProcessPluginCommunicator<TEnvironment: Environment> {
  communicator: tokio::sync::RwLock<Arc<ProcessPluginCommunicator>>,
  restart_info: ProcessRestartInfo<TEnvironment>,
}

impl<TEnvironment: Environment> InitializedProcessPluginCommunicator<TEnvironment> {
  pub async fn new(
    plugin_name: String,
    executable_file_path: PathBuf,
    config: (GlobalConfiguration, ConfigKeyMap),
    environment: TEnvironment,
    plugin_collection: Arc<PluginsCollection<TEnvironment>>,
  ) -> Result<Self> {
    let restart_info = ProcessRestartInfo {
      environment,
      plugin_name,
      executable_file_path,
      config,
      plugin_collection,
    };
    let communicator = create_new_communicator(&restart_info).await?;
    let initialized_communicator = Self {
      communicator: tokio::sync::RwLock::new(Arc::new(communicator)),
      restart_info,
    };

    Ok(initialized_communicator)
  }

  #[cfg(test)]
  pub async fn new_test_plugin_communicator(environment: TEnvironment, collection: Arc<PluginsCollection<TEnvironment>>, plugin_config: ConfigKeyMap) -> Self {
    use crate::plugins::implementations::process::get_file_path_from_name_and_version;
    use crate::plugins::implementations::process::get_test_safe_executable_path;

    let plugin_file_path = get_file_path_from_name_and_version("test-process-plugin", "0.1.0", &environment);
    let test_plugin_file_path = get_test_safe_executable_path(plugin_file_path, &environment);

    Self::new(
      "test-process-plugin".to_string(),
      test_plugin_file_path,
      (Default::default(), plugin_config),
      environment.clone(),
      collection,
    )
    .await
    .unwrap()
  }

  pub async fn get_license_text(&self) -> Result<String> {
    self.get_inner().await.license_text().await
  }

  pub async fn get_resolved_config(&self) -> Result<String> {
    self.get_inner().await.resolved_config(CONFIG_ID).await
  }

  pub async fn get_config_diagnostics(&self) -> Result<Vec<ConfigurationDiagnostic>> {
    self.get_inner().await.config_diagnostics(CONFIG_ID).await
  }

  pub async fn format_text(&self, request: InitializedPluginFormatRequest) -> FormatResult {
    match self
      .get_inner()
      .await
      .format_text(
        request.file_path,
        request.file_text,
        request.range,
        CONFIG_ID,
        request.override_config,
        request.token,
      )
      .await
    {
      Ok(result) => Ok(result),
      Err(err) => {
        // attempt to restart the communicator if this fails and it's no longer alive
        let mut communicator = self.communicator.write().await;
        if communicator.is_process_alive().await {
          Err(err)
        } else {
          *communicator = Arc::new(create_new_communicator(&self.restart_info).await?);
          Err(err)
        }
      }
    }
  }

  pub async fn get_inner(&self) -> Arc<ProcessPluginCommunicator> {
    self.communicator.read().await.clone()
  }
}

async fn create_new_communicator<TEnvironment: Environment>(restart_info: &ProcessRestartInfo<TEnvironment>) -> Result<ProcessPluginCommunicator> {
  // ensure it's initialized each time
  let plugin_name = restart_info.plugin_name.to_string();
  let environment = restart_info.environment.clone();
  let communicator = ProcessPluginCommunicator::new(
    &restart_info.executable_file_path,
    move |error_message| {
      environment.log_stderr_with_context(&error_message, &plugin_name);
    },
    restart_info.plugin_collection.clone(),
  )
  .await?;
  communicator.register_config(CONFIG_ID, &restart_info.config.0, &restart_info.config.1).await?;
  Ok(communicator)
}

#[cfg(test)]
mod test {
  use std::time::Duration;

  use dprint_core::plugins::NullCancellationToken;
  use tokio_util::sync::CancellationToken;

  use super::*;
  use crate::environment::TestEnvironmentBuilder;

  #[test]
  fn should_handle_killing_process_plugin() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_process_plugin().build();
    environment.run_in_runtime({
      let environment = environment.clone();
      async move {
        let collection = PluginsCollection::new(environment.clone());
        // ensure that the config gets recreated as well
        let mut config = ConfigKeyMap::new();
        config.insert("ending".to_string(), "custom".to_string().into());
        let communicator = InitializedProcessPluginCommunicator::new_test_plugin_communicator(environment.clone(), Arc::new(collection), config).await;

        // ensure basic formatting works
        {
          let formatted_text = communicator
            .format_text(InitializedPluginFormatRequest {
              file_path: PathBuf::from("test.txt"),
              file_text: "testing".to_string(),
              range: None,
              override_config: Default::default(),
              token: Arc::new(NullCancellationToken),
            })
            .await
            .unwrap();
          assert_eq!(formatted_text, Some("testing_custom".to_string()));
        }

        // now start up a few formats that will never finish
        let mut futures = Vec::new();
        for _ in 0..10 {
          futures.push(communicator.format_text(InitializedPluginFormatRequest {
            file_path: PathBuf::from("test.txt"),
            // special text that makes it wait for cancellation
            file_text: "wait_cancellation".to_string(),
            range: None,
            override_config: Default::default(),
            token: Arc::new(NullCancellationToken),
          }));
        }

        // spawn a task to kill the process plugin after a bit of time
        let inner_communicator = communicator.get_inner().await;
        tokio::task::spawn(async move {
          // give everything some time to queue up then kill the process
          tokio::time::sleep(Duration::from_millis(100)).await;
          inner_communicator.kill();
        });

        // get all the results and they should be error messages
        let results = futures::future::join_all(futures).await;
        for result in results {
          assert_eq!(result.err().unwrap().to_string(), "Sending message failed because the process plugin failed.");
        }

        // ensure we can still format with the original config
        {
          let formatted_text = communicator
            .format_text(InitializedPluginFormatRequest {
              file_path: PathBuf::from("test.txt"),
              file_text: "testing".to_string(),
              range: None,
              override_config: Default::default(),
              token: Arc::new(NullCancellationToken),
            })
            .await
            .unwrap();
          assert_eq!(formatted_text, Some("testing_custom".to_string()));
        }

        assert_eq!(environment.take_stderr_messages(), vec!["Error reading stdout message. early eof"],);
      }
    })
  }

  #[test]
  fn should_handle_cancellation() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_process_plugin().build();
    environment.run_in_runtime({
      let environment = environment.clone();
      async move {
        let collection = PluginsCollection::new(environment.clone());
        let communicator =
          InitializedProcessPluginCommunicator::new_test_plugin_communicator(environment.clone(), Arc::new(collection), Default::default()).await;

        // start up a format that will wait for cancellation
        let token = Arc::new(CancellationToken::new());
        let future = communicator.format_text(InitializedPluginFormatRequest {
          file_path: PathBuf::from("test.txt"),
          // special text that makes it wait for cancellation
          file_text: "wait_cancellation".to_string(),
          range: None,
          override_config: Default::default(),
          token: token.clone(),
        });

        // spawn a task to wait a bit and then cancel the token
        tokio::task::spawn(async move {
          // give everything some time to queue up
          tokio::time::sleep(Duration::from_millis(100)).await;
          token.cancel();
        });

        // drive the future forward in the meantime and get the result
        let result = future.await;

        // should return Ok(None)
        assert_eq!(result.unwrap(), None);
      }
    })
  }
}
