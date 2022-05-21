/// Functionality to exit the process when a panic occurs.
/// This automatically happens when handling process plugin messages.
pub fn setup_exit_process_panic_hook() {
  // tokio doesn't exit on task panic, so implement that behaviour here
  let orig_hook = std::panic::take_hook();
  std::panic::set_hook(Box::new(move |panic_info| {
    orig_hook(panic_info);
    std::process::exit(1);
  }));
}
