//! The messages of schema version 5 of the editor service.

use dprint_core::communication::MessageReader;
use std::io::ErrorKind;
use std::io::Read;
use std::io::Result;
use std::path::PathBuf;

use dprint_core::plugins::FormatRange;

use super::super::messages::MessageBody;
use super::super::messages::MessageBuilder;

// only the tests name the whole message
#[cfg(test)]
pub type EditorMessage = super::super::messages::EditorMessage<EditorMessageBody>;

#[derive(Debug)]
pub enum EditorMessageBody {
  Success(u32),
  Error(u32, Vec<u8>),
  Close,
  IsAlive,
  CanFormat(PathBuf),
  CanFormatResponse(u32, u32),
  Format(FormatEditorMessageBody),
  FormatResponse(u32, Option<Vec<u8>>),
  CancelFormat(u32),
  #[allow(dead_code)]
  Unknown(u32, Vec<u8>),
}

impl MessageBody for EditorMessageBody {
  fn read<TRead: Read + Unpin>(reader: &mut MessageReader<TRead>, kind: u32, body_length: u32) -> Result<Self> {
    Ok(match kind {
      0 => EditorMessageBody::Success(reader.read_u32()?),
      1 => {
        let message_id = reader.read_u32()?;
        let data = reader.read_sized_bytes()?;
        EditorMessageBody::Error(message_id, data)
      }
      2 => EditorMessageBody::Close,
      3 => EditorMessageBody::IsAlive,
      4 => {
        let file_path = reader.read_sized_bytes()?;
        EditorMessageBody::CanFormat(PathBuf::from(String::from_utf8_lossy(&file_path).to_string()))
      }
      5 => {
        let message_id = reader.read_u32()?;
        let can_format = reader.read_u32()?;
        EditorMessageBody::CanFormatResponse(message_id, can_format)
      }
      6 => {
        let (file_path, range, override_config, file_bytes) = read_format_body(reader)?;
        EditorMessageBody::Format(FormatEditorMessageBody {
          file_path,
          range,
          file_bytes,
          override_config,
        })
      }
      7 => {
        let message_id = reader.read_u32()?;
        let response_kind = reader.read_u32()?;
        let data = match response_kind {
          0 => None,
          1 => Some(reader.read_sized_bytes()?),
          _ => {
            return Err(std::io::Error::new(
              ErrorKind::InvalidData,
              format!("Unknown format response kind: {}", response_kind),
            ));
          }
        };
        EditorMessageBody::FormatResponse(message_id, data)
      }
      8 => EditorMessageBody::CancelFormat(reader.read_u32()?),
      _ => {
        let data = reader.read_bytes(body_length as usize)?;
        EditorMessageBody::Unknown(kind, data)
      }
    })
  }

  fn write<'a>(&'a self, builder: &mut MessageBuilder<'a>) {
    match self {
      EditorMessageBody::Success(message_id) => {
        builder.add_number(*message_id);
      }
      EditorMessageBody::Error(message_id, data) => {
        builder.add_number(*message_id);
        builder.add_bytes(data);
      }
      EditorMessageBody::Close => {}
      EditorMessageBody::IsAlive => {}
      EditorMessageBody::CanFormat(path_buf) => {
        let path = path_buf.to_string_lossy().to_string();
        builder.add_owned_bytes(path.into_bytes());
      }
      EditorMessageBody::CanFormatResponse(message_id, can_format) => {
        builder.add_number(*message_id);
        builder.add_number(*can_format);
      }
      EditorMessageBody::Format(body) => {
        add_format_body(builder, &body.file_path, &body.range, &body.override_config, &body.file_bytes);
      }
      EditorMessageBody::FormatResponse(message_id, data) => {
        builder.add_number(*message_id);
        match &data {
          None => {
            builder.add_number(0);
          }
          Some(data) => {
            builder.add_number(1);
            builder.add_bytes(data);
          }
        }
      }
      EditorMessageBody::CancelFormat(message_id) => {
        builder.add_number(*message_id);
      }
      EditorMessageBody::Unknown(_, _) => unreachable!(), // should never be written
    }
  }

  fn kind(&self) -> u32 {
    match self {
      EditorMessageBody::Success(_) => 0,
      EditorMessageBody::Error(_, _) => 1,
      EditorMessageBody::Close => 2,
      EditorMessageBody::IsAlive => 3,
      EditorMessageBody::CanFormat(_) => 4,
      EditorMessageBody::CanFormatResponse(_, _) => 5,
      EditorMessageBody::Format(_) => 6,
      EditorMessageBody::FormatResponse(_, _) => 7,
      EditorMessageBody::CancelFormat(_) => 8,
      EditorMessageBody::Unknown(_, _) => unreachable!(),
    }
  }

  fn error(message_id: u32, data: Vec<u8>) -> Self {
    EditorMessageBody::Error(message_id, data)
  }
}

#[derive(Debug)]
pub struct FormatEditorMessageBody {
  pub file_path: PathBuf,
  pub range: FormatRange,
  pub override_config: Vec<u8>,
  pub file_bytes: Vec<u8>,
}

/// Reads the file path, range, data in between and file text of a format message.
fn read_format_body<TRead: Read + Unpin>(reader: &mut MessageReader<TRead>) -> Result<(PathBuf, FormatRange, Vec<u8>, Vec<u8>)> {
  let file_path = reader.read_sized_bytes()?;
  let start_byte_index = reader.read_u32()?;
  let end_byte_index = reader.read_u32()?;
  let data = reader.read_sized_bytes()?;
  let file_text = reader.read_sized_bytes()?;
  // clamp the range to the text because the plugins index into the text with it
  let end_byte_index = std::cmp::min(end_byte_index as usize, file_text.len());
  let start_byte_index = std::cmp::min(start_byte_index as usize, end_byte_index);
  let range = if start_byte_index == 0 && end_byte_index == file_text.len() {
    None
  } else {
    Some(std::ops::Range {
      start: start_byte_index,
      end: end_byte_index,
    })
  };
  Ok((PathBuf::from(String::from_utf8_lossy(&file_path).to_string()), range, data, file_text))
}

fn add_format_body<'a>(builder: &mut MessageBuilder<'a>, file_path: &std::path::Path, range: &FormatRange, data: &'a [u8], file_bytes: &'a [u8]) {
  let path = file_path.to_string_lossy().to_string();
  builder.add_owned_bytes(path.into_bytes());
  builder.add_number(range.as_ref().map(|r| r.start as u32).unwrap_or(0));
  builder.add_number(range.as_ref().map(|r| r.end as u32).unwrap_or(file_bytes.len() as u32));
  builder.add_bytes(data);
  builder.add_bytes(file_bytes);
}
