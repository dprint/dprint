mod process;
mod public;
mod wasm;

pub use public::*;
pub use wasm::WASMER_COMPILER_VERSION;

pub use wasm::compile as compile_wasm;

#[cfg(test)]
mod test {
  use std::path::PathBuf;
  use std::sync::Arc;
  use std::time::Duration;

  use dprint_core::plugins::Host;
  use dprint_core::plugins::HostFormatRequest;
  use tokio_util::sync::CancellationToken;

  use crate::arg_parser::CliArgs;
  use crate::configuration::resolve_config_from_args;
  use crate::environment::TestEnvironmentBuilder;
  use crate::plugins::PluginCache;
  use crate::plugins::PluginResolver;
  use crate::plugins::PluginsCollection;

  #[test]
  fn should_support_host_format_cancellation() {
    let environment = TestEnvironmentBuilder::with_initialized_remote_wasm_and_process_plugin().build();
    environment.run_in_runtime({
      let environment = environment.clone();
      async move {
        let collection = Arc::new(PluginsCollection::new(environment.clone()));
        let plugin_cache = Arc::new(PluginCache::new(environment.clone()));
        let resolver = PluginResolver::new(environment.clone(), plugin_cache, collection.clone());
        let cli_args = CliArgs::empty();
        let config = resolve_config_from_args(&cli_args, &environment).unwrap();
        let mut plugins = resolver.resolve_plugins(config.plugins).await.unwrap();
        assert_eq!(plugins.iter().map(|p| p.name()).collect::<Vec<_>>(), vec!["test-plugin", "test-process-plugin"]);
        for plugin in plugins.iter_mut() {
          plugin.set_config(Default::default(), Default::default());
        }
        collection.set_plugins(plugins, &config.base_path).unwrap();

        let token = Arc::new(CancellationToken::new());
        tokio::task::spawn({
          let token = token.clone();
          async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            token.cancel();
          }
        });
        let result = collection
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

        collection.drop_and_shutdown_initialized().await;
        assert_eq!(result, None);
      }
    });
  }
}
