use std::time::Duration;
use std::thread;

/// Starts a thread that polls for the existence of the parent process.
/// If the parent process no longer exists, then it will exit the current process.
pub fn start_parent_process_checker_thread(current_process_name: String, parent_process_id: u32) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_secs(30));

            if !is_process_active(parent_process_id) {
                eprintln!("[{}]: Parent process lost. Exiting.", current_process_name);
                std::process::exit(1);
            }
        }
    })
}

fn is_process_active(process_id: u32) -> bool {
    use sysinfo::{SystemExt, RefreshKind};
    let system = sysinfo::System::new_with_specifics(RefreshKind::new().with_processes());

    // this seems silly
    #[cfg(target_os="windows")]
    let process_id = process_id as usize;
    #[cfg(not(target_os="windows"))]
    let process_id = process_id as i32;

    system.get_process(process_id).is_some()
}
