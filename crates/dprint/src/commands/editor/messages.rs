//! What the messages of every schema version of the editor service share.

use dprint_core::communication::Message;
use dprint_core::communication::MessageReader;
use dprint_core::communication::MessageWriter;
use std::io::Read;
use std::io::Result;
use std::io::Write;

/// A message between the editor and the editor service, where the body is one
/// of the messages of a schema version.
#[derive(Debug)]
pub struct EditorMessage<TBody: MessageBody> {
  pub id: u32,
  pub body: TBody,
}

impl<TBody: MessageBody> EditorMessage<TBody> {
  pub fn read<TRead: Read + Unpin>(reader: &mut MessageReader<TRead>) -> Result<Self> {
    let id = reader.read_u32()?;
    let kind = reader.read_u32()?;
    let body_length = reader.read_u32()?;
    let body = TBody::read(reader, kind, body_length)?;
    reader.read_success_bytes()?;
    Ok(EditorMessage { id, body })
  }
}

impl<TBody: MessageBody> Message for EditorMessage<TBody> {
  fn write<TWrite: Write + Unpin>(&self, writer: &mut MessageWriter<TWrite>) -> Result<()> {
    let mut builder = MessageBuilder::new(self.id, self.body.kind());
    self.body.write(&mut builder);
    builder.write(writer)
  }
}

/// The messages of a schema version of the editor service.
pub trait MessageBody: std::fmt::Debug + Send + Sync + Sized + 'static {
  /// Reads the body of a message of the kind, which is `body_length` bytes.
  fn read<TRead: Read + Unpin>(reader: &mut MessageReader<TRead>, kind: u32, body_length: u32) -> Result<Self>;
  /// Adds the body to the message that's being written.
  fn write<'a>(&'a self, builder: &mut MessageBuilder<'a>);
  /// The kind of message, which decides what the body is.
  fn kind(&self) -> u32;
  /// The response to a message that couldn't be handled.
  fn error(message_id: u32, data: Vec<u8>) -> Self;
}

enum NumberOrBytes<'a> {
  Number(u32),
  Bytes(&'a [u8]),
  OwnedBytes(Vec<u8>),
}

pub struct MessageBuilder<'a> {
  message_id: u32,
  kind: u32,
  parts: Vec<NumberOrBytes<'a>>,
}

impl<'a> MessageBuilder<'a> {
  pub fn new(message_id: u32, kind: u32) -> Self {
    Self {
      message_id,
      kind,
      parts: Vec::new(),
    }
  }

  pub fn add_number(&mut self, num: u32) {
    self.parts.push(NumberOrBytes::Number(num));
  }

  pub fn add_bytes(&mut self, bytes: &'a [u8]) {
    self.parts.push(NumberOrBytes::Bytes(bytes));
  }

  pub fn add_owned_bytes(&mut self, bytes: Vec<u8>) {
    self.parts.push(NumberOrBytes::OwnedBytes(bytes));
  }

  pub fn write<TWrite: Write + Unpin>(&mut self, writer: &mut MessageWriter<TWrite>) -> Result<()> {
    writer.send_u32(self.message_id)?;
    writer.send_u32(self.kind)?;
    writer.send_u32(self.body_size())?;
    for part in &self.parts {
      match part {
        NumberOrBytes::Number(val) => writer.send_u32(*val)?,
        NumberOrBytes::Bytes(bytes) => writer.send_sized_bytes(bytes)?,
        NumberOrBytes::OwnedBytes(bytes) => writer.send_sized_bytes(bytes)?,
      }
    }
    writer.send_success_bytes()?;
    Ok(())
  }

  fn body_size(&self) -> u32 {
    self
      .parts
      .iter()
      .map(|p| match p {
        NumberOrBytes::Number(_) => 4,
        NumberOrBytes::Bytes(bytes) => bytes.len() + 4,
        NumberOrBytes::OwnedBytes(bytes) => bytes.len() + 4,
      })
      .sum::<usize>() as u32
  }
}
