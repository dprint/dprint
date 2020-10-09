use std::sync::{Arc, Mutex, MutexGuard};

/// Lock that can be used throughout an application to synchronize output
/// to the console.
#[derive(Clone)]
pub struct OutputLock {
    output_lock: Arc<Mutex<()>>,
}

impl OutputLock {
    pub fn new() -> Self {
        OutputLock {
            output_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn unwrap_lock<'a>(&'a self) -> MutexGuard<'a, ()> {
        self.output_lock.lock().unwrap()
    }
}
