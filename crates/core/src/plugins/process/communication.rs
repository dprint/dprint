use anyhow::Result;
use std::io::Read;
use std::io::Write;

const SUCCESS_BYTES: &[u8; 4] = &[255, 255, 255, 255];

pub struct StdinReader<TRead: Read> {
  reader: TRead,
}

impl<TRead: Read> StdinReader<TRead> {
  pub fn new(reader: TRead) -> Self {
    Self { reader }
  }

  /// Reads a u32 value.
  pub fn read_u32(&mut self) -> u32 {
    let mut int_buf: [u8; 4] = [0; 4];
    self.reader.read_exact(&mut int_buf).unwrap();
    u32::from_be_bytes(int_buf)
  }

  /// Reads a u32 value followed by a buffer.
  pub fn read_sized_bytes(&mut self) -> Vec<u8> {
    let size = self.read_u32() as usize;
    self.read_bytes(size)
  }

  pub fn read_bytes(&mut self, size: usize) -> Vec<u8> {
    let mut buf = Vec::with_capacity(size);
    unsafe {
      buf.set_len(size);
    }
    self.reader.read_exact(&mut buf).unwrap();
    buf
  }

  pub fn read_success_bytes(&mut self) {
    let read_bytes = self.inner_read_success_bytes();
    if &read_bytes != SUCCESS_BYTES {
      panic!(
        "Catastrophic error reading from process. Did not receive the success bytes at end of message. Found: {:?}",
        read_bytes
      )
    }
  }

  pub fn read_success_bytes_with_message_on_error(&mut self, maybe_read_error_message: &[u8]) {
    let read_bytes = self.inner_read_success_bytes();
    if &read_bytes != SUCCESS_BYTES {
      let message = "Catastrophic error reading from process. Did not receive the success bytes at end of message.";
      // attempt to convert the error message to a string
      match std::str::from_utf8(maybe_read_error_message) {
        Ok(error_message) => panic!("{} Found: {:?}. Received partial error: {}", message, read_bytes, error_message),
        Err(_) => panic!("{}", message),
      }
    }
  }

  fn inner_read_success_bytes(&mut self) -> [u8; 4] {
    let mut read_buf: [u8; 4] = [0; 4];
    self.reader.read_exact(&mut read_buf).unwrap();
    read_buf
  }
}

pub struct StdoutWriter<TWrite: Write> {
  writer: TWrite,
}

impl<TWrite: Write> StdoutWriter<TWrite> {
  pub fn new(writer: TWrite) -> Self {
    StdoutWriter { writer }
  }

  /// Send a u32 value.
  pub fn send_u32(&mut self, value: u32) {
    self.writer.write_all(&value.to_be_bytes()).unwrap();
  }

  pub fn send_sized_bytes(&mut self, bytes: &[u8]) {
    self.send_u32(bytes.len() as u32);
    self.writer.write_all(bytes).unwrap();
  }

  pub fn send_success_bytes(&mut self) {
    self.writer.write_all(SUCCESS_BYTES).unwrap();
    self.writer.flush().unwrap();
  }
}
