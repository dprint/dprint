use crate::plugins::FormatConfig;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

pub struct PluginId(u16);

pub struct DirectoryTreeScope {
  /// Plugins ordered by precedence.
  pub plugins: Vec<PluginId>,
  /// File paths that should be formatted for this scope.
  pub file_paths_by_plugins: HashMap<PluginId, Vec<PathBuf>>,
  /// The raw configuration for each plugin.
  pub config: HashMap<PluginId, Arc<FormatConfig>>,
}
