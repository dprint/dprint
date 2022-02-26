use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use parking_lot::Mutex;
use serde::Serialize;

use crate::configuration::ResolveConfigurationResult;

#[derive(Default)]
struct ProcessContextInner<TConfiguration: Serialize + Clone> {
  configurations: HashMap<u32, Arc<ResolveConfigurationResult<TConfiguration>>>,
}

#[derive(Default)]
pub struct ProcessContext<TConfiguration: Serialize + Clone>(Arc<Mutex<ProcessContextInner<TConfiguration>>>);

impl<TConfiguration: Serialize + Clone> ProcessContext<TConfiguration> {
  pub fn store_config_result(&self, id: u32, result: ResolveConfigurationResult<TConfiguration>) {
    self.0.lock().configurations.insert(id, Arc::new(result));
  }

  pub fn get_config_result(&self, id: u32) -> Option<Arc<ResolveConfigurationResult<TConfiguration>>> {
    self.0.lock().configurations.get(&id).cloned()
  }
}
