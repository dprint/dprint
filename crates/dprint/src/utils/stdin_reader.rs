use anyhow::Result;
use std::io::Read;
use std::io::{self};

#[cfg(test)]
pub use tests::TestStdInReader;

pub trait StdInReader: Clone + Send + Sync {
  fn read(&self) -> Result<Vec<u8>>;
}

#[derive(Default, Clone, Copy)]
pub struct RealStdInReader;

impl StdInReader for RealStdInReader {
  fn read(&self) -> Result<Vec<u8>> {
    let mut text = Vec::new();
    io::stdin().read_to_end(&mut text)?;
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
    text: Arc<Mutex<Option<Vec<u8>>>>,
  }

  impl<S: ToString> From<S> for TestStdInReader {
    fn from(value: S) -> Self {
      Self {
        text: Arc::new(Mutex::new(Some(value.to_string().into_bytes()))),
      }
    }
  }

  impl StdInReader for TestStdInReader {
    fn read(&self) -> Result<Vec<u8>> {
      let text = self.text.lock();
      Ok(text.as_ref().expect("Expected to have stdin text set.").clone())
    }
  }
}
