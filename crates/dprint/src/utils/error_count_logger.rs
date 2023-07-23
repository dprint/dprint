use crate::environment::Environment;
use std::cell::RefCell;
use std::rc::Rc;

/// Logger that keeps track of how many errors it's logged.
#[derive(Clone)]
pub struct ErrorCountLogger<TEnvironment: Environment> {
  error_count: Rc<RefCell<usize>>,
  environment: TEnvironment,
}

impl<TEnvironment: Environment> ErrorCountLogger<TEnvironment> {
  pub fn from_environment(environment: &TEnvironment) -> Self {
    ErrorCountLogger {
      error_count: Rc::new(RefCell::new(0)),
      environment: environment.clone(),
    }
  }

  pub fn log_error(&self, message: &str) {
    self.environment.log_stderr(message);
    self.add_error_count(1);
  }

  pub fn add_error_count(&self, count: usize) {
    *self.error_count.borrow_mut() += count;
  }

  pub fn get_error_count(&self) -> usize {
    *self.error_count.borrow()
  }
}
