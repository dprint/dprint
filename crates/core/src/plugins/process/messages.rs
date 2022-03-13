use std::path::PathBuf;

use anyhow::bail;
use anyhow::Result;
use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;

use crate::plugins::FormatRange;

use super::communication::MessageReader;
use super::communication::MessageWriter;

#[derive(Debug)]
pub struct Message {
  pub id: u32,
  pub body: MessageBody,
}

impl Message {
  pub async fn read<TRead: AsyncRead + Unpin>(reader: &mut MessageReader<TRead>) -> Result<Message> {
    let id = reader.read_u32().await?;
    let message_kind = reader.read_u32().await?;
    let body = match message_kind {
      1 => MessageBody::Close,
      2 => MessageBody::IsAlive,
      3 => MessageBody::GetPluginInfo,
      4 => MessageBody::GetLicenseText,
      5 => {
        let config_id = reader.read_u32().await?;
        let global_config = reader.read_sized_bytes().await?;
        let plugin_config = reader.read_sized_bytes().await?;
        MessageBody::RegisterConfig(RegisterConfigMessageBody {
          config_id,
          global_config,
          plugin_config,
        })
      }
      6 => MessageBody::ReleaseConfig(reader.read_u32().await?),
      7 => MessageBody::GetConfigDiagnostics(reader.read_u32().await?),
      8 => MessageBody::GetResolvedConfig(reader.read_u32().await?),
      9 => {
        let file_path = reader.read_sized_bytes().await?;
        let start_byte_index = reader.read_u32().await?;
        let end_byte_index = reader.read_u32().await?;
        let config_id = reader.read_u32().await?;
        let override_config = reader.read_sized_bytes().await?;
        let file_text = reader.read_sized_bytes().await?;
        MessageBody::FormatText(FormatTextMessageBody {
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
      10 => MessageBody::CancelFormat(reader.read_u32().await?),
      11 => {
        let response_kind = reader.read_u32().await?;
        MessageBody::HostFormatResponse(match response_kind {
          0 => HostFormatResponseMessageBody::NoChange,
          1 => HostFormatResponseMessageBody::Change(reader.read_sized_bytes().await?),
          2 => HostFormatResponseMessageBody::Error(reader.read_sized_bytes().await?),
          _ => bail!("Unknown response kind: {}", response_kind),
        })
      }
      _ => {
        bail!("Unknown message kind: {}", message_kind)
      }
    };
    reader.read_success_bytes().await?;
    Ok(Message { id, body })
  }

  pub async fn write<TWrite: AsyncWrite + Unpin>(&self, writer: &mut MessageWriter<TWrite>) -> Result<()> {
    writer.send_u32(self.id).await?;
    match &self.body {
      MessageBody::Close => {
        writer.send_u32(1).await?;
      }
      MessageBody::IsAlive => {
        writer.send_u32(2).await?;
      }
      MessageBody::GetPluginInfo => {
        writer.send_u32(3).await?;
      }
      MessageBody::GetLicenseText => {
        writer.send_u32(4).await?;
      }
      MessageBody::RegisterConfig(body) => {
        writer.send_u32(5).await?;
        writer.send_u32(body.config_id).await?;
        writer.send_sized_bytes(&body.global_config).await?;
        writer.send_sized_bytes(&body.plugin_config).await?;
      }
      MessageBody::ReleaseConfig(config_id) => {
        writer.send_u32(6).await?;
        writer.send_u32(*config_id).await?;
      }
      MessageBody::GetConfigDiagnostics(config_id) => {
        writer.send_u32(7).await?;
        writer.send_u32(*config_id).await?;
      }
      MessageBody::GetResolvedConfig(config_id) => {
        writer.send_u32(8).await?;
        writer.send_u32(*config_id).await?;
      }
      MessageBody::FormatText(body) => {
        writer.send_u32(9).await?;
        writer.send_sized_bytes(&body.file_path.to_string_lossy().as_bytes()).await?;
        writer.send_u32(body.range.as_ref().map(|r| r.start).unwrap_or(0) as u32).await?;
        writer
          .send_u32(body.range.as_ref().map(|r| r.end).unwrap_or(body.file_text.len()) as u32)
          .await?;
        writer.send_u32(body.config_id).await?;
        writer.send_sized_bytes(&body.override_config).await?;
        writer.send_sized_bytes(&body.file_text).await?;
      }
      MessageBody::CancelFormat(message_id) => {
        writer.send_u32(10).await?;
        writer.send_u32(*message_id).await?;
      }
      MessageBody::HostFormatResponse(body) => {
        writer.send_u32(11).await?;
        match body {
          HostFormatResponseMessageBody::NoChange => writer.send_u32(0).await?,
          HostFormatResponseMessageBody::Change(text) => {
            writer.send_u32(1).await?;
            writer.send_sized_bytes(text).await?;
          }
          HostFormatResponseMessageBody::Error(text) => {
            writer.send_u32(2).await?;
            writer.send_sized_bytes(text).await?;
          }
        }
      }
    }
    writer.send_success_bytes().await?;
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
  pub range: FormatRange,
  pub config_id: u32,
  pub override_config: Vec<u8>,
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
  pub async fn write<TWrite: AsyncWrite + Unpin>(&self, writer: &mut MessageWriter<TWrite>) -> Result<()> {
    writer.send_u32(self.id).await?;
    match &self.body {
      ResponseBody::Success(body) => {
        writer.send_u32(0).await?;
        match body {
          ResponseSuccessBody::Acknowledge => {
            // do nothing, success bytes will be sent
          }
          ResponseSuccessBody::Data(data) => {
            writer.send_sized_bytes(&data).await?;
          }
          ResponseSuccessBody::FormatText(maybe_text) => match maybe_text {
            None => {
              writer.send_u32(0).await?;
            }
            Some(text) => {
              writer.send_u32(1).await?;
              writer.send_sized_bytes(text).await?;
            }
          },
        }
      }
      ResponseBody::Error(text) => {
        writer.send_u32(1).await?;
        writer.send_sized_bytes(&text.as_bytes()).await?;
      }
      ResponseBody::HostFormat(body) => {
        writer.send_u32(2).await?;
        writer.send_sized_bytes(&body.file_path.to_string_lossy().as_bytes()).await?;
        writer.send_u32(body.range.as_ref().map(|r| r.start).unwrap_or(0) as u32).await?;
        writer
          .send_u32(body.range.as_ref().map(|r| r.end).unwrap_or(body.file_text.len()) as u32)
          .await?;
        writer.send_sized_bytes(&body.override_config).await?;
        writer.send_sized_bytes(&body.file_text).await?;
      }
    }
    writer.send_success_bytes().await?;
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
  pub range: FormatRange,
  pub override_config: Vec<u8>,
  pub file_text: Vec<u8>,
}

pub enum ResponseSuccessBody {
  Acknowledge,
  Data(Vec<u8>),
  FormatText(Option<Vec<u8>>),
}
