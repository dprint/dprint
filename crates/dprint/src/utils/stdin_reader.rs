use dprint_core::types::ErrBox;
use std::io::{self, Read};

#[cfg(test)]
pub use tests::TestStdInReader;

pub trait StdInReader: Clone + Send + Sync {
  fn read(&self) -> Result<String, ErrBox>;
}

#[derive(Default, Clone, Copy)]
pub struct RealStdInReader;

impl StdInReader for RealStdInReader {
  fn read(&self) -> Result<String, ErrBox> {
    let mut text = String::new();
    io::stdin().read_to_string(&mut text)?;
    Ok(text)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use parking_lot::Mutex;
  use std::sync::Arc;

  #[derive(Default, Clone)]
  pub struct TestStdInReader {
    text: Arc<Mutex<Option<String>>>,
  }

  impl<S: ToString> From<S> for TestStdInReader {
    fn from(value: S) -> Self {
      Self {
        text: Arc::new(Mutex::new(Some(value.to_string()))),
      }
    }
  }

  impl StdInReader for TestStdInReader {
    fn read(&self) -> Result<String, ErrBox> {
      let text = self.text.lock();
      Ok(text.as_ref().expect("Expected to have stdin text set.").clone())
    }
  }
}
