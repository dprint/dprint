use super::MessagePart;
use super::StdIoReaderWriter;
use crate::types::ErrBox;
use std::io::Read;
use std::io::Write;
use std::path::PathBuf;

pub struct ReadMessageParts {
  parts: Vec<Vec<u8>>,
}

impl ReadMessageParts {
  pub fn take_path_buf(&mut self) -> Result<PathBuf, ErrBox> {
    let message_data = self.take_part()?;
    Ok(PathBuf::from(String::from_utf8(message_data)?))
  }

  pub fn take_string(&mut self) -> Result<String, ErrBox> {
    let message_data = self.take_part()?;
    Ok(String::from_utf8(message_data)?)
  }

  pub fn take_part(&mut self) -> Result<Vec<u8>, ErrBox> {
    if self.parts.is_empty() {
      err!("Programming error: Expected to take message part.")
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
    StdIoMessenger { reader_writer }
  }

  pub fn read_code(&mut self) -> Result<u32, ErrBox> {
    self.reader_writer.read_u32()
  }

  pub fn read_multi_part_message(&mut self, part_count: u32) -> Result<ReadMessageParts, ErrBox> {
    let mut parts = Vec::with_capacity(part_count as usize);
    for _ in 0..part_count {
      parts.push(self.reader_writer.read_variable_data()?);
    }
    self.reader_writer.read_success_bytes()?;
    Ok(ReadMessageParts { parts })
  }

  // TODO: GET RID OF THESE AND JUST USE INTO

  pub fn read_single_part_path_buf_message(&mut self) -> Result<PathBuf, ErrBox> {
    let message = self.read_single_part_message()?;
    let text = String::from_utf8(message)?;
    Ok(PathBuf::from(text))
  }

  pub fn read_single_part_string_message(&mut self) -> Result<String, ErrBox> {
    let message = self.read_single_part_message()?;
    Ok(String::from_utf8(message)?)
  }

  pub fn read_single_part_error_message(&mut self) -> Result<String, ErrBox> {
    let message = self.reader_writer.read_variable_data()?;
    self.reader_writer.read_success_bytes_with_message_on_error(&message)?;
    Ok(String::from_utf8(message)?)
  }

  pub fn read_single_part_u32_message(&mut self) -> Result<u32, ErrBox> {
    let data = self.reader_writer.read_u32()?;
    self.reader_writer.read_success_bytes()?;
    Ok(data)
  }

  pub fn read_single_part_message(&mut self) -> Result<Vec<u8>, ErrBox> {
    let data = self.reader_writer.read_variable_data()?;
    self.reader_writer.read_success_bytes()?;
    Ok(data)
  }

  pub fn read_zero_part_message(&mut self) -> Result<(), ErrBox> {
    self.reader_writer.read_success_bytes()
  }

  pub fn send_message(&mut self, code: u32, message_parts: Vec<MessagePart>) -> Result<(), ErrBox> {
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
