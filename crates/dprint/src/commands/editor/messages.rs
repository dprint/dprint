use anyhow::bail;
use anyhow::Result;
use dprint_core::communication::Message;
use dprint_core::communication::MessageReader;
use dprint_core::communication::MessageWriter;
use std::io::Read;
use std::io::Write;
use std::path::PathBuf;

use dprint_core::plugins::FormatRange;

#[derive(Debug)]
pub struct EditorMessage {
  pub id: u32,
  pub body: EditorMessageBody,
}

impl EditorMessage {
  pub fn read<TRead: Read + Unpin>(reader: &mut MessageReader<TRead>) -> Result<EditorMessage> {
    let id = reader.read_u32()?;
    let message_kind = reader.read_u32()?;
    let body_length = reader.read_u32()?;
    let body = match message_kind {
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
        let file_path = reader.read_sized_bytes()?;
        let start_byte_index = reader.read_u32()?;
        let end_byte_index = reader.read_u32()?;
        let override_config = reader.read_sized_bytes()?;
        let file_text = reader.read_sized_bytes()?;
        EditorMessageBody::Format(FormatEditorMessageBody {
          file_path: PathBuf::from(String::from_utf8_lossy(&file_path).to_string()),
          range: if start_byte_index == 0 && end_byte_index == file_text.len() as u32 {
            None
          } else {
            Some(std::ops::Range {
              start: start_byte_index as usize,
              end: end_byte_index as usize,
            })
          },
          file_text,
          override_config,
        })
      }
      7 => {
        let message_id = reader.read_u32()?;
        let response_kind = reader.read_u32()?;
        let data = match response_kind {
          0 => None,
          1 => Some(reader.read_sized_bytes()?),
          _ => bail!("Unknown format response kind: {}", response_kind),
        };
        EditorMessageBody::FormatResponse(message_id, data)
      }
      8 => EditorMessageBody::CancelFormat(reader.read_u32()?),
      _ => {
        let data = reader.read_bytes(body_length as usize)?;
        EditorMessageBody::Unknown(message_kind, data)
      }
    };
    reader.read_success_bytes()?;
    Ok(EditorMessage { id, body })
  }
}

impl Message for EditorMessage {
  fn write<TWrite: Write + Unpin>(&self, writer: &mut MessageWriter<TWrite>) -> Result<()> {
    writer.send_u32(self.id)?;
    match &self.body {
      EditorMessageBody::Success(message_id) => {
        writer.send_u32(0)?;
        writer.send_u32(4)?; // body size
        writer.send_u32(*message_id)?;
      }
      EditorMessageBody::Error(message_id, data) => {
        writer.send_u32(1)?;
        writer.send_u32(4 + data.len() as u32)?; // body size
        writer.send_u32(*message_id)?;
        writer.send_sized_bytes(data)?;
      }
      EditorMessageBody::Close => {
        writer.send_u32(2)?;
        writer.send_u32(0)?; // body size
      }
      EditorMessageBody::IsAlive => {
        writer.send_u32(3)?;
        writer.send_u32(0)?; // body size
      }
      EditorMessageBody::CanFormat(path_buf) => {
        let path = path_buf.to_string_lossy().to_string();
        writer.send_u32(4)?;
        writer.send_u32(4 + path.len() as u32)?; // body size
        writer.send_sized_bytes(path.as_bytes())?;
      }
      EditorMessageBody::CanFormatResponse(message_id, can_format) => {
        writer.send_u32(5)?;
        writer.send_u32(4 * 2)?; // body size
        writer.send_u32(*message_id)?;
        writer.send_u32(*can_format)?;
      }
      EditorMessageBody::Format(body) => {
        let path = body.file_path.to_string_lossy().to_string();
        writer.send_u32(6)?;
        writer.send_u32(4 + path.len() as u32 + 4 * 2 + 4 + body.override_config.len() as u32 + 4 + body.file_text.len() as u32)?; // body size
        writer.send_sized_bytes(path.as_bytes())?;
        writer.send_u32(body.range.as_ref().map(|r| r.start as u32).unwrap_or(0))?;
        writer.send_u32(body.range.as_ref().map(|r| r.end as u32).unwrap_or_else(|| body.file_text.len() as u32))?;
        writer.send_sized_bytes(&body.override_config)?;
        writer.send_sized_bytes(&body.file_text)?;
      }
      EditorMessageBody::FormatResponse(message_id, data) => {
        writer.send_u32(7)?;
        writer.send_u32(4 + 4 + data.as_ref().map(|d| d.len()).unwrap_or(0) as u32)?; // body size
        writer.send_u32(*message_id)?;
        match &data {
          None => {
            writer.send_u32(0)?;
          }
          Some(data) => {
            writer.send_u32(1)?;
            writer.send_sized_bytes(data)?;
          }
        }
      }
      EditorMessageBody::CancelFormat(message_id) => {
        writer.send_u32(8)?;
        writer.send_u32(4)?; // body size
        writer.send_u32(*message_id)?;
      }
      EditorMessageBody::Unknown(_, _) => unreachable!(), // should never be written
    }
    writer.send_success_bytes()?;
    Ok(())
  }
}

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
  Unknown(u32, Vec<u8>),
}

#[derive(Debug)]
pub struct FormatEditorMessageBody {
  pub file_path: PathBuf,
  pub range: FormatRange,
  pub override_config: Vec<u8>,
  pub file_text: Vec<u8>,
}
