use async_trait::async_trait;
use super::{Plugin, PluginSourceReference};
use crate::types::ErrBox;

#[async_trait(?Send)]
pub trait PluginResolver {
    async fn resolve_plugins(&self, plugin_references: Vec<PluginSourceReference>) -> Result<Vec<Box<dyn Plugin>>, ErrBox>;
}
