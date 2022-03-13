use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use parking_lot::Mutex;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::configuration::ConfigKeyMap;
use crate::configuration::ConfigurationDiagnostic;
use crate::configuration::GlobalConfiguration;

pub type FormatHostSender = tokio::sync::oneshot::Sender<Result<Option<String>>>;

pub struct StoredConfig<TConfiguration: Serialize + Clone> {
  pub config: Arc<TConfiguration>,
  pub diagnostics: Arc<Vec<ConfigurationDiagnostic>>,
  pub config_map: ConfigKeyMap,
  pub global_config: GlobalConfiguration,
}

struct ProcessContextInner<TConfiguration: Serialize + Clone> {
  configurations: HashMap<u32, Arc<StoredConfig<TConfiguration>>>,
  cancellation_tokens: HashMap<u32, Arc<CancellationToken>>,
  format_host_id_count: u32,
  format_host_senders: HashMap<u32, FormatHostSender>,
}

#[derive(Clone)]
pub struct ProcessContext<TConfiguration: Serialize + Clone>(Arc<Mutex<ProcessContextInner<TConfiguration>>>);

impl<TConfiguration: Serialize + Clone> ProcessContext<TConfiguration> {
  pub fn new() -> Self {
    // for some reason, `#[derive(Default)]` wasn't working
    ProcessContext(Arc::new(Mutex::new(ProcessContextInner {
      configurations: Default::default(),
      cancellation_tokens: Default::default(),
      format_host_id_count: 0,
      format_host_senders: Default::default(),
    })))
  }

  pub fn store_config_result(&self, id: u32, config: StoredConfig<TConfiguration>) {
    let mut data = self.0.lock();
    data.configurations.insert(id, Arc::new(config));
  }

  pub fn release_config_result(&self, id: u32) {
    let mut data = self.0.lock();
    data.configurations.remove(&id);
  }

  pub fn get_config(&self, id: u32) -> Option<Arc<StoredConfig<TConfiguration>>> {
    self.0.lock().configurations.get(&id).cloned()
  }

  pub fn store_cancellation_token(&self, id: u32, token: Arc<CancellationToken>) {
    self.0.lock().cancellation_tokens.insert(id, token);
  }

  pub fn cancel_format(&self, id: u32) {
    let token = self.0.lock().cancellation_tokens.remove(&id);
    if let Some(token) = token {
      token.cancel();
    }
  }

  pub fn release_cancellation_token(&self, id: u32) {
    self.0.lock().cancellation_tokens.remove(&id);
  }

  pub fn store_format_host_sender(&self, sender: FormatHostSender) -> u32 {
    let mut data = self.0.lock();
    let id = data.format_host_id_count;
    data.format_host_id_count += 1;
    data.format_host_senders.insert(id, sender);
    id
  }

  pub fn take_format_host_sender(&self, id: u32) -> Option<FormatHostSender> {
    self.0.lock().format_host_senders.remove(&id)
  }
}
