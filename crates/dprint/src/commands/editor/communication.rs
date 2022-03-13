// The code used in here is legacy and will be phased out from the editor service in the future

use anyhow::bail;
use anyhow::Result;
use std::borrow::Cow;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

const BUFFER_SIZE: usize = 1024; // safe to assume

const SUCCESS_BYTES: &[u8; 4] = &[255, 255, 255, 255];
// todo: unit tests

pub struct StdIoReaderWriter<TRead: Read, TWrite: Write> {
  writer: TWrite,
  reader: TRead,
}

impl<TRead: Read, TWrite: Write> StdIoReaderWriter<TRead, TWrite> {
  pub fn new(reader: TRead, writer: TWrite) -> Self {
    StdIoReaderWriter { writer, reader }
  }

  /// Send a u32 value.
  pub fn send_u32(&mut self, value: u32) -> Result<()> {
    self.writer.write_all(&value.to_be_bytes())?;

    Ok(())
  }

  /// Reads a u32 value.
  pub fn read_u32(&mut self) -> Result<u32> {
    let mut int_buf: [u8; 4] = [0; 4];
    self.reader.read_exact(&mut int_buf)?;
    Ok(u32::from_be_bytes(int_buf))
  }

  pub fn send_success_bytes(&mut self) -> Result<()> {
    self.writer.write_all(SUCCESS_BYTES)?;
    self.writer.flush()?;

    Ok(())
  }

  pub fn read_success_bytes(&mut self) -> Result<()> {
    let read_bytes = self.inner_read_success_bytes()?;
    if &read_bytes == SUCCESS_BYTES {
      Ok(())
    } else {
      panic!(
        "Catastrophic error reading from process. Did not receive the success bytes at end of message. Found: {:?}",
        read_bytes
      )
    }
  }

  pub fn read_success_bytes_with_message_on_error(&mut self, maybe_read_error_message: &[u8]) -> Result<()> {
    let read_bytes = self.inner_read_success_bytes()?;
    if &read_bytes == SUCCESS_BYTES {
      Ok(())
    } else {
      let message = "Catastrophic error reading from process. Did not receive the success bytes at end of message.";
      // attempt to convert the error message to a string
      match std::str::from_utf8(maybe_read_error_message) {
        Ok(error_message) => panic!("{} Found: {:?}. Received partial error: {}", message, read_bytes, error_message),
        Err(_) => panic!("{}", message),
      }
    }
  }

  fn inner_read_success_bytes(&mut self) -> Result<[u8; 4]> {
    let mut read_buf: [u8; 4] = [0; 4];
    self.reader.read_exact(&mut read_buf)?;
    Ok(read_buf)
  }

  /// Sends variable width data (4 bytes length, X bytes data)
  pub fn send_variable_data(&mut self, data: &[u8]) -> Result<()> {
    // send the message part length (4 bytes)
    self.writer.write_all(&(data.len() as u32).to_be_bytes())?;

    // write first part of data to writer buffer
    self.writer.write_all(&data[0..std::cmp::min(BUFFER_SIZE, data.len())])?;
    self.writer.flush()?;

    // write remaining bytes
    let mut index = BUFFER_SIZE;
    while index < data.len() {
      // wait for "ready" from the client
      self.reader.read_exact(&mut [0; 4])?;

      // write to buffer
      let start_index = index;
      let end_index = std::cmp::min(index + BUFFER_SIZE, data.len());
      self.writer.write_all(&data[start_index..end_index])?;
      self.writer.flush()?;

      index += BUFFER_SIZE;
    }

    Ok(())
  }

  /// Gets the message part (4 bytes length, X bytes data)
  /// Messages may have multiple parts.
  pub fn read_variable_data(&mut self) -> Result<Vec<u8>> {
    let size = self.read_u32()? as usize;

    let mut message_data = vec![0u8; size];
    if size > 0 {
      // read first part of response
      self.reader.read_exact(&mut message_data[0..std::cmp::min(BUFFER_SIZE, size)])?;

      // read remaining bytes
      let mut index = BUFFER_SIZE;
      while index < size {
        // send "ready" to the client
        self.writer.write_all(&[0; 4])?;
        self.writer.flush()?;

        // read from buffer
        let start_index = index;
        let end_index = std::cmp::min(index + BUFFER_SIZE, size);
        self.reader.read_exact(&mut message_data[start_index..end_index])?;

        index += BUFFER_SIZE;
      }
    }

    Ok(message_data)
  }
}

pub struct ReadMessageParts {
  parts: Vec<Vec<u8>>,
}

impl ReadMessageParts {
  pub fn take_path_buf(&mut self) -> Result<PathBuf> {
    let message_data = self.take_part()?;
    Ok(PathBuf::from(String::from_utf8(message_data)?))
  }

  pub fn take_string(&mut self) -> Result<String> {
    let message_data = self.take_part()?;
    Ok(String::from_utf8(message_data)?)
  }

  pub fn take_part(&mut self) -> Result<Vec<u8>> {
    if self.parts.is_empty() {
      bail!("Programming error: Expected to take message part.")
    } else {
      Ok(self.parts.remove(0))
    }
  }
}

/// Uses an StdIoReaderWriter to send and receive multi-part messages.
pub struct StdIoMessenger<TRead: Read, TWrite: Write> {
  reader_writer: StdIoReaderWriter<TRead, TWrite>,
}

impl<TRead: Read, TWrite: Write> StdIoMessenger<TRead, TWrite> {
  pub fn new(reader_writer: StdIoReaderWriter<TRead, TWrite>) -> Self {
    Self { reader_writer }
  }

  pub fn read_code(&mut self) -> Result<u32> {
    self.reader_writer.read_u32()
  }

  pub fn read_multi_part_message(&mut self, part_count: u32) -> Result<ReadMessageParts> {
    let mut parts = Vec::with_capacity(part_count as usize);
    for _ in 0..part_count {
      parts.push(self.reader_writer.read_variable_data()?);
    }
    self.reader_writer.read_success_bytes()?;
    Ok(ReadMessageParts { parts })
  }

  // TODO: GET RID OF THESE AND JUST USE INTO

  pub fn read_single_part_path_buf_message(&mut self) -> Result<PathBuf> {
    let message = self.read_single_part_message()?;
    let text = String::from_utf8(message)?;
    Ok(PathBuf::from(text))
  }

  pub fn read_single_part_message(&mut self) -> Result<Vec<u8>> {
    let data = self.reader_writer.read_variable_data()?;
    self.reader_writer.read_success_bytes()?;
    Ok(data)
  }

  #[cfg(test)]
  pub fn read_single_part_string_message(&mut self) -> Result<String> {
    let message = self.read_single_part_message()?;
    Ok(String::from_utf8(message)?)
  }

  #[cfg(test)]
  pub fn read_single_part_error_message(&mut self) -> Result<String> {
    let message = self.reader_writer.read_variable_data()?;
    self.reader_writer.read_success_bytes_with_message_on_error(&message)?;
    Ok(String::from_utf8(message)?)
  }

  #[cfg(test)]
  pub fn read_zero_part_message(&mut self) -> Result<()> {
    self.reader_writer.read_success_bytes()
  }

  pub fn send_message(&mut self, code: u32, message_parts: Vec<MessagePart>) -> Result<()> {
    self.reader_writer.send_u32(code)?;
    for message_part in message_parts {
      match message_part {
        MessagePart::Number(value) => self.reader_writer.send_u32(value)?,
        MessagePart::VariableData(value) => self.reader_writer.send_variable_data(&value)?,
      }
    }
    self.reader_writer.send_success_bytes()?;

    Ok(())
  }
}

pub enum MessagePart<'a> {
  VariableData(Cow<'a, [u8]>),
  Number(u32),
}

impl<'a> From<&'a Path> for MessagePart<'a> {
  fn from(value: &'a Path) -> Self {
    match value.to_string_lossy() {
      Cow::Owned(value) => value.into(),
      Cow::Borrowed(value) => value.into(),
    }
  }
}

impl<'a> From<String> for MessagePart<'a> {
  fn from(value: String) -> Self {
    MessagePart::VariableData(Cow::Owned(value.into_bytes()))
  }
}

impl<'a> From<&'a str> for MessagePart<'a> {
  fn from(value: &'a str) -> Self {
    MessagePart::VariableData(Cow::Borrowed(value.as_bytes()))
  }
}

impl<'a> From<Cow<'a, str>> for MessagePart<'a> {
  fn from(value: Cow<'a, str>) -> Self {
    match value {
      Cow::Owned(value) => value.into(),
      Cow::Borrowed(value) => value.into(),
    }
  }
}

impl<'a> From<&'a [u8]> for MessagePart<'a> {
  fn from(value: &'a [u8]) -> Self {
    MessagePart::VariableData(Cow::Borrowed(value))
  }
}

impl<'a> From<&'a Vec<u8>> for MessagePart<'a> {
  fn from(value: &'a Vec<u8>) -> Self {
    MessagePart::VariableData(Cow::Borrowed(value))
  }
}

impl<'a> From<Vec<u8>> for MessagePart<'a> {
  fn from(value: Vec<u8>) -> Self {
    MessagePart::VariableData(Cow::Owned(value))
  }
}

impl<'a> From<u32> for MessagePart<'a> {
  fn from(value: u32) -> Self {
    MessagePart::Number(value)
  }
}
