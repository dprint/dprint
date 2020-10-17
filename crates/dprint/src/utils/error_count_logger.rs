use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use crate::environment::Environment;

/// Logger that keeps track of how many errors it's logged.
#[derive(Clone)]
pub struct ErrorCountLogger<TEnvironment: Environment> {
    error_count: Arc<AtomicUsize>,
    environment: TEnvironment,
}

impl<TEnvironment: Environment> ErrorCountLogger<TEnvironment> {
    pub fn from_environment(environment: &TEnvironment) -> Self {
        ErrorCountLogger {
            error_count: Arc::new(AtomicUsize::new(0)),
            environment: environment.clone(),
        }
    }

    pub fn log_error(&self, message: &str) {
        self.environment.log_error(message);
        self.error_count.fetch_add(1, Ordering::SeqCst);
    }

    pub fn get_error_count(&self) -> usize {
        self.error_count.load(Ordering::SeqCst)
    }
}
