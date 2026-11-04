use std::io::ErrorKind;
use std::io::Read;
use std::io::Result;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;
use serde::Serialize;

use crate::communication::Message;
use crate::plugins::ConfigChange;
use crate::plugins::FormatConfigId;
use crate::plugins::FormatRange;
use crate::plugins::FormatRanges;

use crate::communication::MessageReader;
use crate::communication::MessageWriter;

pub type MessageId = u32;

mod message_ids {
  use super::MessageId;

  pub const SUCCESS_ID: MessageId = 0;
  pub const DATA_RESPONSE_ID: MessageId = 1;
  pub const ERROR_ID: MessageId = 2;
  pub const CLOSE_ID: MessageId = 3;
  pub const IS_ALIVE_ID: MessageId = 4;
  pub const GET_PLUGIN_INFO_ID: MessageId = 5;
  pub const GET_LICENSE_TEXT_ID: MessageId = 6;
  pub const REGISTER_CONFIG_ID: MessageId = 7;
  pub const RELEASE_CONFIG_ID: MessageId = 8;
  pub const GET_CONFIG_DIAGNOSTICS_ID: MessageId = 9;
  pub const GET_FILE_MATCHING_INFO_ID: MessageId = 10;
  pub const GET_RESOLVED_CONFIG_ID: MessageId = 11;
  pub const CHECK_CONFIG_UPDATES_ID: MessageId = 12;
  pub const FORMAT_ID: MessageId = 13;
  pub const FORMAT_RESPONSE_ID: MessageId = 14;
  pub const CANCEL_FORMAT_ID: MessageId = 15;
  pub const HOST_FORMAT_ID: MessageId = 16;
  pub const FORMAT_RANGES_ID: MessageId = 17;
}

#[derive(Debug)]
pub struct ProcessPluginMessage {
  pub id: MessageId,
  pub body: MessageBody,
}

impl ProcessPluginMessage {
  pub fn read<TRead: Read + Unpin>(reader: &mut MessageReader<TRead>) -> Result<ProcessPluginMessage> {
    let id = reader.read_u32()?;
    let message_kind = reader.read_u32()?;
    let body = match message_kind {
      message_ids::SUCCESS_ID => MessageBody::Success(reader.read_u32()?),
      message_ids::DATA_RESPONSE_ID => {
        let message_id = reader.read_u32()?;
        let data = reader.read_sized_bytes()?;
        MessageBody::DataResponse(ResponseBody { message_id, data })
      }
      message_ids::ERROR_ID => {
        let message_id = reader.read_u32()?;
        let data = reader.read_sized_bytes()?;
        MessageBody::Error(ResponseBody { message_id, data })
      }
      message_ids::CLOSE_ID => MessageBody::Close,
      message_ids::IS_ALIVE_ID => MessageBody::IsAlive,
      message_ids::GET_PLUGIN_INFO_ID => MessageBody::GetPluginInfo,
      message_ids::GET_LICENSE_TEXT_ID => MessageBody::GetLicenseText,
      message_ids::REGISTER_CONFIG_ID => {
        let config_id = FormatConfigId::from_raw(reader.read_u32()?);
        let global_config = reader.read_sized_bytes()?;
        let plugin_config = reader.read_sized_bytes()?;
        MessageBody::RegisterConfig(RegisterConfigMessageBody {
          config_id,
          global_config,
          plugin_config,
        })
      }
      message_ids::RELEASE_CONFIG_ID => MessageBody::ReleaseConfig(FormatConfigId::from_raw(reader.read_u32()?)),
      message_ids::GET_CONFIG_DIAGNOSTICS_ID => MessageBody::GetConfigDiagnostics(FormatConfigId::from_raw(reader.read_u32()?)),
      message_ids::GET_FILE_MATCHING_INFO_ID => MessageBody::GetFileMatchingInfo(FormatConfigId::from_raw(reader.read_u32()?)),
      message_ids::GET_RESOLVED_CONFIG_ID => MessageBody::GetResolvedConfig(FormatConfigId::from_raw(reader.read_u32()?)),
      message_ids::CHECK_CONFIG_UPDATES_ID => {
        let body_bytes = reader.read_sized_bytes()?;
        MessageBody::CheckConfigUpdates(body_bytes)
      }
      message_ids::FORMAT_ID => {
        let file_path = reader.read_sized_bytes()?;
        let start_byte_index = reader.read_u32()?;
        let end_byte_index = reader.read_u32()?;
        let config_id = FormatConfigId::from_raw(reader.read_u32()?);
        let override_config = reader.read_sized_bytes()?;
        let file_text = reader.read_sized_bytes()?;
        MessageBody::Format(FormatMessageBody {
          file_path: PathBuf::from(String::from_utf8_lossy(&file_path).to_string()),
          ranges: if start_byte_index == 0 && end_byte_index == file_text.len() as u32 {
            None
          } else {
            Some(vec![std::ops::Range {
              start: start_byte_index as usize,
              end: end_byte_index as usize,
            }])
          },
          config_id,
          file_bytes: file_text,
          override_config,
        })
      }
      message_ids::FORMAT_RANGES_ID => {
        let file_path = reader.read_sized_bytes()?;
        let ranges_len = reader.read_u32()?;
        let mut ranges = Vec::with_capacity(ranges_len as usize);
        for _ in 0..ranges_len {
          let start_byte_index = reader.read_u32()?;
          let end_byte_index = reader.read_u32()?;
          ranges.push(std::ops::Range {
            start: start_byte_index as usize,
            end: end_byte_index as usize,
          });
        }
        let config_id = FormatConfigId::from_raw(reader.read_u32()?);
        let override_config = reader.read_sized_bytes()?;
        let file_text = reader.read_sized_bytes()?;
        MessageBody::Format(FormatMessageBody {
          file_path: PathBuf::from(String::from_utf8_lossy(&file_path).to_string()),
          ranges: Some(ranges),
          config_id,
          file_bytes: file_text,
          override_config,
        })
      }
      message_ids::FORMAT_RESPONSE_ID => {
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
        MessageBody::FormatResponse(ResponseBody { message_id, data })
      }
      message_ids::CANCEL_FORMAT_ID => MessageBody::CancelFormat(reader.read_u32()?),
      message_ids::HOST_FORMAT_ID => {
        let original_message_id = reader.read_u32()?;
        let file_path = reader.read_sized_bytes()?;
        let start_byte_index = reader.read_u32()?;
        let end_byte_index = reader.read_u32()?;
        let override_config = reader.read_sized_bytes()?;
        let file_text = reader.read_sized_bytes()?;
        MessageBody::HostFormat(HostFormatMessageBody {
          original_message_id,
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
        return Ok(ProcessPluginMessage {
          id,
          body: MessageBody::Unknown(message_kind),
        });
      }
    };
    reader.read_success_bytes()?;
    Ok(ProcessPluginMessage { id, body })
  }
}

impl Message for ProcessPluginMessage {
  fn write<TWrite: Write + Unpin>(&self, writer: &mut MessageWriter<TWrite>) -> Result<()> {
    writer.send_u32(self.id)?;
    match &self.body {
      MessageBody::Success(message_id) => {
        writer.send_u32(message_ids::SUCCESS_ID)?;
        writer.send_u32(*message_id)?;
      }
      MessageBody::DataResponse(response) => {
        writer.send_u32(message_ids::DATA_RESPONSE_ID)?;
        writer.send_u32(response.message_id)?;
        writer.send_sized_bytes(&response.data)?;
      }
      MessageBody::Error(response) => {
        writer.send_u32(message_ids::ERROR_ID)?;
        writer.send_u32(response.message_id)?;
        writer.send_sized_bytes(&response.data)?;
      }
      MessageBody::Close => {
        writer.send_u32(message_ids::CLOSE_ID)?;
      }
      MessageBody::IsAlive => {
        writer.send_u32(message_ids::IS_ALIVE_ID)?;
      }
      MessageBody::GetPluginInfo => {
        writer.send_u32(message_ids::GET_PLUGIN_INFO_ID)?;
      }
      MessageBody::GetLicenseText => {
        writer.send_u32(message_ids::GET_LICENSE_TEXT_ID)?;
      }
      MessageBody::RegisterConfig(body) => {
        writer.send_u32(message_ids::REGISTER_CONFIG_ID)?;
        writer.send_u32(body.config_id.as_raw())?;
        writer.send_sized_bytes(&body.global_config)?;
        writer.send_sized_bytes(&body.plugin_config)?;
      }
      MessageBody::ReleaseConfig(config_id) => {
        writer.send_u32(message_ids::RELEASE_CONFIG_ID)?;
        writer.send_u32(config_id.as_raw())?;
      }
      MessageBody::GetConfigDiagnostics(config_id) => {
        writer.send_u32(message_ids::GET_CONFIG_DIAGNOSTICS_ID)?;
        writer.send_u32(config_id.as_raw())?;
      }
      MessageBody::GetFileMatchingInfo(config_id) => {
        writer.send_u32(message_ids::GET_FILE_MATCHING_INFO_ID)?;
        writer.send_u32(config_id.as_raw())?;
      }
      MessageBody::GetResolvedConfig(config_id) => {
        writer.send_u32(message_ids::GET_RESOLVED_CONFIG_ID)?;
        writer.send_u32(config_id.as_raw())?;
      }
      MessageBody::CheckConfigUpdates(body_bytes) => {
        writer.send_u32(message_ids::CHECK_CONFIG_UPDATES_ID)?;
        writer.send_sized_bytes(body_bytes)?;
      }
      MessageBody::Format(body) => body.write(writer, &body.file_bytes)?,
      MessageBody::FormatShared(body) => body.write(writer, &body.file_bytes)?,
      MessageBody::FormatResponse(response) => {
        writer.send_u32(message_ids::FORMAT_RESPONSE_ID)?;
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
        writer.send_u32(message_ids::CANCEL_FORMAT_ID)?;
        writer.send_u32(*message_id)?;
      }
      MessageBody::HostFormat(body) => {
        writer.send_u32(message_ids::HOST_FORMAT_ID)?;
        writer.send_u32(body.original_message_id)?;
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
  Success(MessageId),
  DataResponse(ResponseBody<Vec<u8>>),
  Error(ResponseBody<Vec<u8>>),
  Close,
  IsAlive,
  GetPluginInfo,
  GetLicenseText,
  RegisterConfig(RegisterConfigMessageBody),
  ReleaseConfig(FormatConfigId),
  GetConfigDiagnostics(FormatConfigId),
  GetFileMatchingInfo(FormatConfigId),
  GetResolvedConfig(FormatConfigId),
  CheckConfigUpdates(Vec<u8>),
  Format(FormatMessageBody),
  /// A format message the host sends, which is read by the plugin as a
  /// `Format` message. The bytes are shared so that the host can send
  /// them without copying them when its caller keeps them.
  FormatShared(FormatMessageBody<Arc<Vec<u8>>>),
  FormatResponse(ResponseBody<Option<Vec<u8>>>),
  CancelFormat(MessageId),
  HostFormat(HostFormatMessageBody),
  /// If encountered, process plugin should panic and
  /// the CLI should kill the process plugin.
  Unknown(u32),
}

#[derive(Debug)]
pub struct ResponseBody<T: std::fmt::Debug> {
  pub message_id: MessageId,
  pub data: T,
}

#[derive(Debug)]
pub struct RegisterConfigMessageBody {
  pub config_id: FormatConfigId,
  pub global_config: Vec<u8>,
  pub plugin_config: Vec<u8>,
}

pub type CheckConfigUpdatesMessageBody = crate::plugins::CheckConfigUpdatesMessage;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckConfigUpdatesResponseBody {
  pub changes: Vec<ConfigChange>,
}

#[derive(Debug)]
pub struct FormatMessageBody<TFileBytes = Vec<u8>> {
  pub file_path: PathBuf,
  pub ranges: FormatRanges,
  pub config_id: FormatConfigId,
  pub override_config: Vec<u8>,
  pub file_bytes: TFileBytes,
}

impl<TFileBytes> FormatMessageBody<TFileBytes> {
  /// Writes a `Format` message, which every plugin reads, unless there are
  /// several ranges, which is a `FormatRanges` message that only a plugin
  /// with the `multiRangeFormat` capability reads.
  fn write<TWrite: Write + Unpin>(&self, writer: &mut MessageWriter<TWrite>, file_bytes: &[u8]) -> Result<()> {
    match self.ranges.as_deref() {
      Some(ranges) if ranges.len() != 1 => {
        writer.send_u32(message_ids::FORMAT_RANGES_ID)?;
        writer.send_sized_bytes(self.file_path.to_string_lossy().as_bytes())?;
        writer.send_u32(ranges.len() as u32)?;
        for range in ranges {
          writer.send_u32(range.start as u32)?;
          writer.send_u32(range.end as u32)?;
        }
      }
      ranges => {
        let range = ranges.and_then(|ranges| ranges.first());
        writer.send_u32(message_ids::FORMAT_ID)?;
        writer.send_sized_bytes(self.file_path.to_string_lossy().as_bytes())?;
        writer.send_u32(range.map(|r| r.start).unwrap_or(0) as u32)?;
        writer.send_u32(range.map(|r| r.end).unwrap_or(file_bytes.len()) as u32)?;
      }
    }
    writer.send_u32(self.config_id.as_raw())?;
    writer.send_sized_bytes(&self.override_config)?;
    writer.send_sized_bytes(file_bytes)
  }
}

#[derive(Debug)]
pub struct HostFormatMessageBody {
  pub original_message_id: MessageId,
  pub file_path: PathBuf,
  pub range: FormatRange,
  pub override_config: Vec<u8>,
  pub file_text: Vec<u8>,
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  #[allow(clippy::single_range_in_vec_init)]
  fn format_message_ranges_roundtrip() {
    // (message kind, ranges read)
    fn roundtrip(ranges: FormatRanges) -> (u32, FormatRanges) {
      let message = ProcessPluginMessage {
        id: 1,
        body: MessageBody::Format(FormatMessageBody {
          file_path: PathBuf::from("file.ts"),
          ranges,
          config_id: FormatConfigId::from_raw(2),
          override_config: Vec::new(),
          file_bytes: b"0123456789".to_vec(),
        }),
      };
      let mut bytes = Vec::new();
      message.write(&mut MessageWriter::new(&mut bytes)).unwrap();
      let message_kind = u32::from_be_bytes(bytes[4..8].try_into().unwrap());
      let message = ProcessPluginMessage::read(&mut MessageReader::new(std::io::Cursor::new(bytes))).unwrap();
      match message.body {
        MessageBody::Format(body) => {
          assert_eq!(body.file_path, PathBuf::from("file.ts"));
          assert_eq!(body.config_id, FormatConfigId::from_raw(2));
          assert_eq!(body.file_bytes, b"0123456789");
          (message_kind, body.ranges)
        }
        body => panic!("unexpected body: {:?}", body),
      }
    }

    // every plugin reads a format message with one range or the whole text
    assert_eq!(roundtrip(None), (message_ids::FORMAT_ID, None));
    assert_eq!(roundtrip(Some(vec![2..4])), (message_ids::FORMAT_ID, Some(vec![2..4])));
    assert_eq!(roundtrip(Some(vec![0..10])), (message_ids::FORMAT_ID, None));
    // several ranges are a format ranges message
    assert_eq!(roundtrip(Some(vec![1..2, 5..8])), (message_ids::FORMAT_RANGES_ID, Some(vec![1..2, 5..8])));
  }
}
