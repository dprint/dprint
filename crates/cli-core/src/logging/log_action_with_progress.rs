use super::{ProgressBars, ProgressBarStyle};

pub fn log_action_with_progress<
    TResult: std::marker::Send + std::marker::Sync,
    TCreate: FnOnce(Box<dyn Fn(usize)>) -> TResult + std::marker::Send + std::marker::Sync,
>(progress_bars: &Option<ProgressBars>, message: &str, action: TCreate, total_size: usize) -> TResult {
    if let Some(progress_bars) = progress_bars {
        let pb = progress_bars.add_progress(message.to_string(), ProgressBarStyle::Action, total_size);
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
