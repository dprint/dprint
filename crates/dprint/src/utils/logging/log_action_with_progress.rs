use std::sync::Arc;

use super::ProgressBarStyle;
use super::ProgressBars;

pub fn log_action_with_progress<TResult: Send + Sync, TCreate: FnOnce(Box<dyn Fn(usize)>) -> TResult + Send + Sync>(
  progress_bars: &Option<ProgressBars>,
  message: &str,
  action: TCreate,
  total_size: usize,
) -> TResult {
  if let Some(progress_bars) = progress_bars {
    let pb = progress_bars.add_progress(message.to_string(), ProgressBarStyle::Action, total_size);
    let pb = Arc::new(pb);
    let result = action(Box::new({
      let pb = pb.clone();
      move |size| pb.set_position(size)
    }));
    pb.finish();
    result
  } else {
    action(Box::new(|_| { /* do nothing */ }))
  }
}
