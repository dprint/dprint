use dprint_core::types::ErrBox;

pub trait StdInReader: Clone + std::marker::Send + std::marker::Sync + 'static {
  fn read(&self) -> Result<String, ErrBox>;
}

#[derive(Clone)]
pub struct RealStdInReader {}

impl RealStdInReader {
  pub fn new() -> RealStdInReader {
    RealStdInReader {}
  }
}

impl StdInReader for RealStdInReader {
  fn read(&self) -> Result<String, ErrBox> {
    use std::io::{self, Read};
    let mut text = String::new();
    io::stdin().read_to_string(&mut text)?;
    Ok(text)
  }
}

#[derive(Clone)]
#[cfg(test)]
pub struct TestStdInReader {
  text: std::sync::Arc<parking_lot::Mutex<Option<String>>>,
}

#[cfg(test)]
impl TestStdInReader {
  pub fn new() -> TestStdInReader {
    TestStdInReader::new_with_option(None)
  }

  pub fn new_with_text(text: &str) -> TestStdInReader {
    TestStdInReader::new_with_option(Some(text.to_string()))
  }

  fn new_with_option(text: Option<String>) -> TestStdInReader {
    TestStdInReader {
      text: std::sync::Arc::new(parking_lot::Mutex::new(text)),
    }
  }
}

#[cfg(test)]
impl StdInReader for TestStdInReader {
  fn read(&self) -> Result<String, ErrBox> {
    Ok(self.text.lock().as_ref().expect("Expected to have stdin text set.").clone())
  }
}
