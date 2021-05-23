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

/// Gets the parent process id from the CLI arguments.
///
/// The dprint cli will provide a `--parent-pid <value>` flag to specify the parent PID.
pub fn get_parent_process_id_from_cli_args() -> Option<u32> {
    let args: Vec<String> = std::env::args().collect();
    for i in 0..args.len() {
        if args[i] == "--parent-pid" {
            if let Some(parent_pid) = args.get(i + 1) {
                return parent_pid.parse::<u32>().map(Some).unwrap_or(None);
            }
        }
    }

    return None;
}
