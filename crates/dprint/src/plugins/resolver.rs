use async_trait::async_trait;
use super::Plugin;
use crate::types::ErrBox;
use crate::utils::PathSource;

#[async_trait(?Send)]
pub trait PluginResolver {
    async fn resolve_plugins(&self, plugin_sources: &Vec<PathSource>) -> Result<Vec<Box<dyn Plugin>>, ErrBox>;
}
