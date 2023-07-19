use crate::environment::Environment;

struct Factory<TEnvironment: Environment> {
  environment: TEnvironment,
}

impl<TEnvironment: Environment> Factory<TEnvironment> {
  pub fn new(environment: TEnvironment) -> Self {
    Self { environment }
  }
}
