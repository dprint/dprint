use std::sync::Arc;

use anyhow::Result;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::configuration::ConfigKeyMap;
use crate::configuration::ConfigurationDiagnostic;
use crate::configuration::GlobalConfiguration;

use super::stdout_message_writer::StdoutMessageWriter;
use super::utils::ArcIdStore;
use super::utils::IdGenerator;

pub type FormatHostSender = tokio::sync::oneshot::Sender<Result<Option<String>>>;

pub struct StoredConfig<TConfiguration: Serialize + Clone> {
  pub config: Arc<TConfiguration>,
  pub diagnostics: Arc<Vec<ConfigurationDiagnostic>>,
  pub config_map: ConfigKeyMap,
  pub global_config: GlobalConfiguration,
}

#[derive(Clone)]
pub struct ProcessContext<TConfiguration: Serialize + Clone> {
  pub id_generator: IdGenerator,
  pub configs: ArcIdStore<Arc<StoredConfig<TConfiguration>>>,
  pub cancellation_tokens: ArcIdStore<Arc<CancellationToken>>,
  pub format_host_senders: ArcIdStore<FormatHostSender>,
  pub stdout_writer: StdoutMessageWriter,
}

impl<TConfiguration: Serialize + Clone> ProcessContext<TConfiguration> {
  pub fn new(stdout_writer: StdoutMessageWriter) -> Self {
    ProcessContext {
      id_generator: Default::default(),
      configs: ArcIdStore::new(),
      cancellation_tokens: ArcIdStore::new(),
      format_host_senders: ArcIdStore::new(),
      stdout_writer,
    }
  }
}
