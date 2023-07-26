mod process;
mod public;
mod wasm;

pub use public::*;
pub use wasm::WASMER_COMPILER_VERSION;

pub use wasm::compile as compile_wasm;
pub use wasm::WasmModuleCreator;

#[cfg(test)]
mod test {
  use std::path::PathBuf;
  use std::rc::Rc;
  use std::sync::Arc;
  use std::time::Duration;

  use dprint_core::plugins::FormatConfigId;
  use dprint_core::plugins::HostFormatRequest;
  use tokio_util::sync::CancellationToken;

  use crate::arg_parser::CliArgs;
  use crate::configuration::resolve_config_from_args;
  use crate::environment::TestEnvironmentBuilder;
  use crate::plugins::FormatConfig;
  use crate::plugins::PluginCache;
  use crate::plugins::PluginResolver;
  use crate::resolution::PluginWithConfig;
  use crate::resolution::PluginsScope;

  #[test]
  fn should_support_host_format_cancellation() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin().build();
    environment.run_in_runtime({
      let environment = environment.clone();
      async move {
        let plugin_cache = PluginCache::new(environment.clone());
        let resolver = Rc::new(PluginResolver::new(environment.clone(), plugin_cache));
        let cli_args = CliArgs::empty();
        let config = Rc::new(resolve_config_from_args(&cli_args, &environment).await.unwrap());
        let plugins = resolver.resolve_plugins(config.plugins.clone()).await.unwrap();
        assert_eq!(
          plugins.iter().map(|p| &p.info().name).collect::<Vec<_>>(),
          vec!["test-plugin", "test-process-plugin"]
        );
        let mut plugins_with_config = Vec::with_capacity(plugins.len());
        for plugin in plugins {
          let format_config = Arc::new(FormatConfig {
            id: FormatConfigId::from_raw(1),
            global: Default::default(),
            raw: Default::default(),
          });
          let instance = plugin.initialize().await.unwrap();
          let file_matching_info = instance.file_matching_info(format_config.clone()).await.unwrap();
          plugins_with_config.push(Rc::new(PluginWithConfig::new(plugin, None, format_config, file_matching_info)));
        }
        let scope = Rc::new(PluginsScope::new(environment.clone(), plugins_with_config, config).unwrap());
        let token = Arc::new(CancellationToken::new());
        dprint_core::async_runtime::spawn({
          let token = token.clone();
          async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            token.cancel();
          }
        });
        let result = scope
          .format(HostFormatRequest {
            file_path: PathBuf::from("file.txt_ps"),
            // This should cause the process plugin to format with the
            // Wasm plugin which will then try to format with the process plugin
            // and finally it will wait for cancellation to occur
            file_text: "plugin: plugin: wait_cancellation".to_string(),
            range: None,
            override_config: Default::default(),
            token: token.clone(),
          })
          .await
          .unwrap();
        assert_eq!(result, None);
        resolver.clear_and_shutdown_initialized().await;
      }
    });
  }

  #[test]
  fn should_support_shutdown_during_indefinite_host_formatting() {
    // same as above test, but the cancellation token is never cancelled
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin().build();
    environment.run_in_runtime({
      let environment = environment.clone();
      async move {
        let plugin_cache = PluginCache::new(environment.clone());
        let resolver = Rc::new(PluginResolver::new(environment.clone(), plugin_cache));
        let cli_args = CliArgs::empty();
        let config = Rc::new(resolve_config_from_args(&cli_args, &environment).await.unwrap());
        let plugins = resolver.resolve_plugins(config.plugins.clone()).await.unwrap();
        assert_eq!(
          plugins.iter().map(|p| &p.info().name).collect::<Vec<_>>(),
          vec!["test-plugin", "test-process-plugin"]
        );
        let mut plugins_with_config = Vec::with_capacity(plugins.len());
        for plugin in plugins {
          let format_config = Arc::new(FormatConfig {
            id: FormatConfigId::from_raw(1),
            global: Default::default(),
            raw: Default::default(),
          });
          let instance = plugin.initialize().await.unwrap();
          let file_matching_info = instance.file_matching_info(format_config.clone()).await.unwrap();
          plugins_with_config.push(Rc::new(PluginWithConfig::new(plugin, None, format_config, file_matching_info)));
        }
        let scope = Rc::new(PluginsScope::new(environment.clone(), plugins_with_config, config).unwrap());
        let token = Arc::new(CancellationToken::new());

        // start up a format that will hang forever
        dprint_core::async_runtime::spawn(async move {
          scope
            .format(HostFormatRequest {
              file_path: PathBuf::from("file.txt_ps"),
              // This should cause the process plugin to format with the
              // Wasm plugin which will then try to format with the process plugin
              // and then will hang forever
              file_text: "plugin: plugin: wait_cancellation".to_string(),
              range: None,
              override_config: Default::default(),
              token,
            })
            .await
            .unwrap();
        });

        // give it some time to start and hang
        tokio::time::sleep(Duration::from_millis(20)).await;
        resolver.clear_and_shutdown_initialized().await;
      }
    });
  }
}
