use anyhow::Context;
use anyhow::Result;
use std::io::BufRead;
use std::io::Read;
use std::io::{self};

#[cfg(test)]
pub use tests::TestStdInReader;

pub trait StdInReader: Clone + Send + Sync {
  fn read(&self) -> Result<Vec<u8>>;

  /// Reads stdin line by line, skipping blank lines, without buffering the
  /// entire input into a single string. Useful for large lists of file paths.
  fn read_non_empty_lines(&self) -> Result<Vec<String>>;
}

#[derive(Default, Clone, Copy)]
pub struct RealStdInReader;

impl StdInReader for RealStdInReader {
  fn read(&self) -> Result<Vec<u8>> {
    let mut text = Vec::new();
    io::stdin().read_to_end(&mut text)?;
    Ok(text)
  }

  fn read_non_empty_lines(&self) -> Result<Vec<String>> {
    let mut lines = Vec::new();
    for line in io::stdin().lock().lines() {
      let line = line.context("Failed reading line from stdin.")?;
      if !line.is_empty() {
        lines.push(line);
      }
    }
    Ok(lines)
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

    fn read_non_empty_lines(&self) -> Result<Vec<String>> {
      let bytes = self.read()?;
      let text = String::from_utf8(bytes).context("Failed reading stdin as UTF-8.")?;
      Ok(text.lines().filter(|line| !line.is_empty()).map(String::from).collect())
    }
  }
}
