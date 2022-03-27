use std::io::Read;
use std::io::Write;
use std::path::PathBuf;

use anyhow::bail;
use anyhow::Result;

use crate::plugins::FormatRange;

use super::communication::MessageReader;
use super::communication::MessageWriter;

#[derive(Debug)]
pub struct Message {
  pub id: u32,
  pub body: MessageBody,
}

impl Message {
  pub fn read<TRead: Read + Unpin>(reader: &mut MessageReader<TRead>) -> Result<Message> {
    let id = reader.read_u32()?;
    let message_kind = reader.read_u32()?;
    let body = match message_kind {
      0 => MessageBody::Success(reader.read_u32()?),
      1 => {
        let message_id = reader.read_u32()?;
        let data = reader.read_sized_bytes()?;
        MessageBody::DataResponse(ResponseBody { message_id, data })
      }
      2 => {
        let message_id = reader.read_u32()?;
        let data = reader.read_sized_bytes()?;
        MessageBody::Error(ResponseBody { message_id, data })
      }
      3 => MessageBody::Close,
      4 => MessageBody::IsAlive,
      5 => MessageBody::GetPluginInfo,
      6 => MessageBody::GetLicenseText,
      7 => {
        let config_id = reader.read_u32()?;
        let global_config = reader.read_sized_bytes()?;
        let plugin_config = reader.read_sized_bytes()?;
        MessageBody::RegisterConfig(RegisterConfigMessageBody {
          config_id,
          global_config,
          plugin_config,
        })
      }
      8 => MessageBody::ReleaseConfig(reader.read_u32()?),
      9 => MessageBody::GetConfigDiagnostics(reader.read_u32()?),
      10 => MessageBody::GetResolvedConfig(reader.read_u32()?),
      11 => {
        let file_path = reader.read_sized_bytes()?;
        let start_byte_index = reader.read_u32()?;
        let end_byte_index = reader.read_u32()?;
        let config_id = reader.read_u32()?;
        let override_config = reader.read_sized_bytes()?;
        let file_text = reader.read_sized_bytes()?;
        MessageBody::Format(FormatMessageBody {
          file_path: PathBuf::from(String::from_utf8_lossy(&file_path).to_string()),
          range: if start_byte_index == 0 && end_byte_index == file_text.len() as u32 {
            None
          } else {
            Some(std::ops::Range {
              start: start_byte_index as usize,
              end: end_byte_index as usize,
            })
          },
          config_id,
          file_text,
          override_config,
        })
      }
      12 => {
        let message_id = reader.read_u32()?;
        let response_kind = reader.read_u32()?;
        let data = match response_kind {
          0 => None,
          1 => Some(reader.read_sized_bytes()?),
          _ => bail!("Unknown format response kind: {}", response_kind),
        };
        MessageBody::FormatResponse(ResponseBody { message_id, data })
      }
      13 => MessageBody::CancelFormat(reader.read_u32()?),
      14 => {
        let file_path = reader.read_sized_bytes()?;
        let start_byte_index = reader.read_u32()?;
        let end_byte_index = reader.read_u32()?;
        let override_config = reader.read_sized_bytes()?;
        let file_text = reader.read_sized_bytes()?;
        MessageBody::HostFormat(HostFormatMessageBody {
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
      _ => {
        // don't read success bytes... receiving this means that
        // the plugin should exit the process after returning an
        // error or panic
        return Ok(Message {
          id,
          body: MessageBody::Unknown(message_kind),
        });
      }
    };
    reader.read_success_bytes()?;
    Ok(Message { id, body })
  }

  pub fn write<TWrite: Write + Unpin>(&self, writer: &mut MessageWriter<TWrite>) -> Result<()> {
    writer.send_u32(self.id)?;
    match &self.body {
      MessageBody::Success(message_id) => {
        writer.send_u32(0)?;
        writer.send_u32(*message_id)?;
      }
      MessageBody::DataResponse(response) => {
        writer.send_u32(1)?;
        writer.send_u32(response.message_id)?;
        writer.send_sized_bytes(&response.data)?;
      }
      MessageBody::Error(response) => {
        writer.send_u32(2)?;
        writer.send_u32(response.message_id)?;
        writer.send_sized_bytes(&response.data)?;
      }
      MessageBody::Close => {
        writer.send_u32(3)?;
      }
      MessageBody::IsAlive => {
        writer.send_u32(4)?;
      }
      MessageBody::GetPluginInfo => {
        writer.send_u32(5)?;
      }
      MessageBody::GetLicenseText => {
        writer.send_u32(6)?;
      }
      MessageBody::RegisterConfig(body) => {
        writer.send_u32(7)?;
        writer.send_u32(body.config_id)?;
        writer.send_sized_bytes(&body.global_config)?;
        writer.send_sized_bytes(&body.plugin_config)?;
      }
      MessageBody::ReleaseConfig(config_id) => {
        writer.send_u32(8)?;
        writer.send_u32(*config_id)?;
      }
      MessageBody::GetConfigDiagnostics(config_id) => {
        writer.send_u32(9)?;
        writer.send_u32(*config_id)?;
      }
      MessageBody::GetResolvedConfig(config_id) => {
        writer.send_u32(10)?;
        writer.send_u32(*config_id)?;
      }
      MessageBody::Format(body) => {
        writer.send_u32(11)?;
        writer.send_sized_bytes(body.file_path.to_string_lossy().as_bytes())?;
        writer.send_u32(body.range.as_ref().map(|r| r.start).unwrap_or(0) as u32)?;
        writer.send_u32(body.range.as_ref().map(|r| r.end).unwrap_or(body.file_text.len()) as u32)?;
        writer.send_u32(body.config_id)?;
        writer.send_sized_bytes(&body.override_config)?;
        writer.send_sized_bytes(&body.file_text)?;
      }
      MessageBody::FormatResponse(response) => {
        writer.send_u32(12)?;
        writer.send_u32(response.message_id)?;
        match &response.data {
          None => {
            writer.send_u32(0)?;
          }
          Some(data) => {
            writer.send_u32(1)?;
            writer.send_sized_bytes(data)?;
          }
        }
      }
      MessageBody::CancelFormat(message_id) => {
        writer.send_u32(13)?;
        writer.send_u32(*message_id)?;
      }
      MessageBody::HostFormat(body) => {
        writer.send_u32(14)?;
        writer.send_sized_bytes(body.file_path.to_string_lossy().as_bytes())?;
        writer.send_u32(body.range.as_ref().map(|r| r.start).unwrap_or(0) as u32)?;
        writer.send_u32(body.range.as_ref().map(|r| r.end).unwrap_or(body.file_text.len()) as u32)?;
        writer.send_sized_bytes(&body.override_config)?;
        writer.send_sized_bytes(&body.file_text)?;
      }
      MessageBody::Unknown(_) => unreachable!(), // should never be written
    }
    writer.send_success_bytes()?;
    Ok(())
  }
}

#[derive(Debug)]
pub enum MessageBody {
  Success(u32),
  DataResponse(ResponseBody<Vec<u8>>),
  Error(ResponseBody<Vec<u8>>),
  Close,
  IsAlive,
  GetPluginInfo,
  GetLicenseText,
  RegisterConfig(RegisterConfigMessageBody),
  ReleaseConfig(u32),
  GetConfigDiagnostics(u32),
  GetResolvedConfig(u32),
  Format(FormatMessageBody),
  FormatResponse(ResponseBody<Option<Vec<u8>>>),
  CancelFormat(u32),
  HostFormat(HostFormatMessageBody),
  /// If encountered, process plugin should panic and
  /// the CLI should kill the process plugin.
  Unknown(u32),
}

#[derive(Debug)]
pub struct ResponseBody<T: std::fmt::Debug> {
  pub message_id: u32,
  pub data: T,
}

#[derive(Debug)]
pub struct RegisterConfigMessageBody {
  pub config_id: u32,
  pub global_config: Vec<u8>,
  pub plugin_config: Vec<u8>,
}

#[derive(Debug)]
pub struct FormatMessageBody {
  pub file_path: PathBuf,
  pub range: FormatRange,
  pub config_id: u32,
  pub override_config: Vec<u8>,
  pub file_text: Vec<u8>,
}

#[derive(Debug)]
pub struct HostFormatMessageBody {
  pub file_path: PathBuf,
  pub range: FormatRange,
  pub override_config: Vec<u8>,
  pub file_text: Vec<u8>,
}
