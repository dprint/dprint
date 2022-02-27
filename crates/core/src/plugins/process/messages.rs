use std::io::Read;
use std::io::Write;
use std::ops::Range;
use std::path::PathBuf;

use anyhow::bail;
use anyhow::Result;

use super::communication::MessageReader;
use super::communication::MessageWriter;

#[derive(Debug)]
pub struct Message {
  pub id: u32,
  pub body: MessageBody,
}

impl Message {
  pub fn read<TRead: Read>(reader: &mut MessageReader<TRead>) -> Result<Message> {
    let id = reader.read_u32()?;
    let message_kind = reader.read_u32()?;
    let body = match message_kind {
      1 => MessageBody::Close,
      2 => MessageBody::IsAlive,
      3 => MessageBody::GetPluginInfo,
      4 => MessageBody::GetLicenseText,
      5 => {
        let config_id = reader.read_u32()?;
        let global_config = reader.read_sized_bytes()?;
        let plugin_config = reader.read_sized_bytes()?;
        MessageBody::RegisterConfig(RegisterConfigMessageBody {
          config_id,
          global_config,
          plugin_config,
        })
      }
      6 => MessageBody::ReleaseConfig(reader.read_u32()?),
      7 => MessageBody::GetConfigDiagnostics(reader.read_u32()?),
      8 => MessageBody::GetResolvedConfig(reader.read_u32()?),
      9 => {
        let file_path = reader.read_sized_bytes()?;
        let start_byte_index = reader.read_u32()?;
        let end_byte_index = reader.read_u32()?;
        let config_id = reader.read_u32()?;
        let override_config = reader.read_sized_bytes()?;
        let file_text = reader.read_sized_bytes()?;
        MessageBody::FormatText(FormatTextMessageBody {
          file_path: PathBuf::from(String::from_utf8_lossy(&file_path).to_string()),
          range: if start_byte_index == 0 && end_byte_index == file_text.len() as u32 {
            None
          } else {
            Some(Range {
              start: start_byte_index as usize,
              end: end_byte_index as usize,
            })
          },
          config_id,
          file_text,
          override_config: if override_config.is_empty() { None } else { Some(override_config) },
        })
      }
      10 => MessageBody::CancelFormat(reader.read_u32()?),
      11 => {
        let response_kind = reader.read_u32()?;
        MessageBody::HostFormatResponse(match response_kind {
          0 => HostFormatResponseMessageBody::NoChange,
          1 => HostFormatResponseMessageBody::Change(reader.read_sized_bytes()?),
          2 => HostFormatResponseMessageBody::Error(reader.read_sized_bytes()?),
          _ => bail!("Unknown response kind: {}", response_kind),
        })
      }
      _ => {
        bail!("Unknown message kind: {}", message_kind)
      }
    };
    reader.read_success_bytes()?;
    Ok(Message { id, body })
  }

  pub fn write<TWrite: Write>(&self, writer: &mut MessageWriter<TWrite>) -> Result<()> {
    writer.send_u32(self.id)?;
    match &self.body {
      MessageBody::Close => {
        writer.send_u32(1)?;
      }
      MessageBody::IsAlive => {
        writer.send_u32(2)?;
      }
      MessageBody::GetPluginInfo => {
        writer.send_u32(3)?;
      }
      MessageBody::GetLicenseText => {
        writer.send_u32(4)?;
      }
      MessageBody::RegisterConfig(body) => {
        writer.send_u32(5)?;
        writer.send_u32(body.config_id)?;
        writer.send_sized_bytes(&body.global_config)?;
        writer.send_sized_bytes(&body.plugin_config)?;
      }
      MessageBody::ReleaseConfig(config_id) => {
        writer.send_u32(6)?;
        writer.send_u32(*config_id)?;
      }
      MessageBody::GetConfigDiagnostics(config_id) => {
        writer.send_u32(7)?;
        writer.send_u32(*config_id)?;
      }
      MessageBody::GetResolvedConfig(config_id) => {
        writer.send_u32(8)?;
        writer.send_u32(*config_id)?;
      }
      MessageBody::FormatText(body) => {
        writer.send_u32(9)?;
        writer.send_sized_bytes(&body.file_path.to_string_lossy().as_bytes())?;
        writer.send_u32(body.range.as_ref().map(|r| r.start).unwrap_or(0) as u32)?;
        writer.send_u32(body.range.as_ref().map(|r| r.end).unwrap_or(body.file_text.len()) as u32)?;
        writer.send_u32(body.config_id)?;
        if let Some(override_config) = &body.override_config {
          writer.send_sized_bytes(override_config)?;
        } else {
          writer.send_sized_bytes(&Vec::with_capacity(0))?;
        }
        writer.send_sized_bytes(&body.file_text)?;
      }
      MessageBody::CancelFormat(message_id) => {
        writer.send_u32(10)?;
        writer.send_u32(*message_id)?;
      }
      MessageBody::HostFormatResponse(body) => {
        writer.send_u32(11)?;
        todo!();
      }
    }
    writer.send_success_bytes()?;
    Ok(())
  }
}

#[derive(Debug)]
pub enum MessageBody {
  Close,
  IsAlive,
  GetPluginInfo,
  GetLicenseText,
  RegisterConfig(RegisterConfigMessageBody),
  ReleaseConfig(u32),
  GetConfigDiagnostics(u32),
  GetResolvedConfig(u32),
  FormatText(FormatTextMessageBody),
  CancelFormat(u32),
  HostFormatResponse(HostFormatResponseMessageBody),
}

#[derive(Debug)]
pub struct RegisterConfigMessageBody {
  pub config_id: u32,
  pub global_config: Vec<u8>,
  pub plugin_config: Vec<u8>,
}

#[derive(Debug)]
pub struct FormatTextMessageBody {
  pub file_path: PathBuf,
  pub range: Option<Range<usize>>,
  pub config_id: u32,
  pub override_config: Option<Vec<u8>>,
  pub file_text: Vec<u8>,
}

#[derive(Debug)]
pub enum HostFormatResponseMessageBody {
  NoChange,
  Change(Vec<u8>),
  Error(Vec<u8>),
}

pub struct Response {
  pub id: u32,
  pub body: ResponseBody,
}

impl Response {
  // pub fn read<TRead: Read>(reader: &mut MessageReader<TRead>) -> Result<Response> {
  //   let id = reader.read_u32()?;
  //   let kind = reader.read_u32()?;
  //   let body = match kind {
  //     // Success
  //     0 => {}
  //     // Error
  //     1 => {}
  //     // Host Format
  //     2 => {}
  //     _ => {}
  //   };
  //   reader.read_success_bytes()?;
  //   Ok(())
  // }

  pub fn write<TWrite: Write>(&self, writer: &mut MessageWriter<TWrite>) -> Result<()> {
    writer.send_u32(self.id)?;
    match &self.body {
      ResponseBody::Success(body) => {
        writer.send_u32(0)?;
        match body {
          ResponseSuccessBody::Acknowledge => {
            // do nothing, success bytes will be sent
          }
          ResponseSuccessBody::Data(data) => {
            writer.send_sized_bytes(&data)?;
          }
          ResponseSuccessBody::FormatText(maybe_text) => match maybe_text {
            None => {
              writer.send_u32(0)?;
            }
            Some(text) => {
              writer.send_u32(1)?;
              writer.send_sized_bytes(text)?;
            }
          },
        }
      }
      ResponseBody::Error(text) => {
        writer.send_u32(1)?;
        writer.send_sized_bytes(&text.as_bytes())?;
      }
      ResponseBody::HostFormat(body) => {
        writer.send_u32(2)?;
        writer.send_sized_bytes(&body.file_path.to_string_lossy().as_bytes())?;
        writer.send_u32(body.range.as_ref().map(|r| r.start).unwrap_or(0) as u32)?;
        writer.send_u32(body.range.as_ref().map(|r| r.end).unwrap_or(body.file_text.len()) as u32)?;
        if let Some(override_config) = &body.override_config {
          writer.send_sized_bytes(override_config)?;
        } else {
          writer.send_sized_bytes(&Vec::with_capacity(0))?;
        }
        writer.send_sized_bytes(&body.file_text)?;
      }
    }
    writer.send_success_bytes()?;
    Ok(())
  }
}

pub enum ResponseBody {
  Success(ResponseSuccessBody),
  Error(String),
  HostFormat(ResponseBodyHostFormat),
}

pub struct ResponseBodyHostFormat {
  pub file_path: PathBuf,
  pub range: Option<Range<usize>>,
  pub override_config: Option<Vec<u8>>,
  pub file_text: Vec<u8>,
}

pub enum ResponseSuccessBody {
  Acknowledge,
  Data(Vec<u8>),
  FormatText(Option<Vec<u8>>),
}