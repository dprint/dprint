use super::LogLevel;

/// Where what's logged and the progress of what's being done go instead of
/// stderr (ex. to an editor that shows them).
pub trait OutputSink: Send + Sync {
  fn log(&self, level: LogLevel, text: &str);
  /// Starts showing the progress, where a `total_size` of `0` means it isn't
  /// known.
  fn start_progress(&self, id: usize, title: &str, total_size: usize);
  fn update_progress(&self, id: usize, position: usize, total_size: usize);
  fn finish_progress(&self, id: usize);
}
