use std::rc::Rc;
use std::sync::Arc;

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::communication::IdGenerator;
use crate::communication::RcIdStore;
use crate::communication::SingleThreadMessageWriter;
use crate::configuration::ConfigKeyMap;
use crate::configuration::ConfigurationDiagnostic;
use crate::configuration::GlobalConfiguration;
use crate::plugins::FileMatchingInfo;
use crate::plugins::FormatResult;

use super::messages::ProcessPluginMessage;

pub type FormatHostSender = tokio::sync::oneshot::Sender<FormatResult>;

pub struct StoredConfig<TConfiguration: Serialize + Clone> {
  pub config: Arc<TConfiguration>,
  pub diagnostics: Rc<Vec<ConfigurationDiagnostic>>,
  pub file_matching: FileMatchingInfo,
  pub config_map: ConfigKeyMap,
  pub global_config: GlobalConfiguration,
}

pub struct ProcessContext<TConfiguration: Serialize + Clone> {
  pub id_generator: Rc<IdGenerator>,
  pub configs: RcIdStore<Rc<StoredConfig<TConfiguration>>>,
  pub cancellation_tokens: RcIdStore<Arc<CancellationToken>>,
  pub format_host_senders: RcIdStore<FormatHostSender>,
  pub stdout_writer: Rc<SingleThreadMessageWriter<ProcessPluginMessage>>,
}

impl<TConfiguration: Serialize + Clone> ProcessContext<TConfiguration> {
  pub fn new(stdout_writer: SingleThreadMessageWriter<ProcessPluginMessage>) -> Self {
    ProcessContext {
      id_generator: Default::default(),
      configs: Default::default(),
      cancellation_tokens: Default::default(),
      format_host_senders: Default::default(),
      stdout_writer: Rc::new(stdout_writer),
    }
  }
}
