use async_trait::async_trait;
use super::Plugin;
use crate::types::ErrBox;

#[async_trait(?Send)]
pub trait PluginResolver {
    async fn resolve_plugins(&self, urls: &Vec<String>) -> Result<Vec<Box<dyn Plugin>>, ErrBox>;
}
