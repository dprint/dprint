use dprint_core::communication::Message;
use dprint_core::communication::MessageReader;
use dprint_core::communication::MessageWriter;
use std::io::ErrorKind;
use std::io::Read;
use std::io::Result;
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
        // clamp the range to the text because the plugins index into the text with it
        let end_byte_index = std::cmp::min(end_byte_index as usize, file_text.len());
        let start_byte_index = std::cmp::min(start_byte_index as usize, end_byte_index);
        EditorMessageBody::Format(FormatEditorMessageBody {
          file_path: PathBuf::from(String::from_utf8_lossy(&file_path).to_string()),
          range: if start_byte_index == 0 && end_byte_index == file_text.len() {
            None
          } else {
            Some(std::ops::Range {
              start: start_byte_index,
              end: end_byte_index,
            })
          },
          file_bytes: file_text,
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
      9 => EditorMessageBody::Capabilities(reader.read_sized_bytes()?),
      10 => {
        let message_id = reader.read_u32()?;
        let data = reader.read_sized_bytes()?;
        EditorMessageBody::CapabilitiesResponse(message_id, data)
      }
      11 => EditorMessageBody::ApproveProcessPlugins(reader.read_sized_bytes()?),
      12 => {
        let message_id = reader.read_u32()?;
        let data = reader.read_sized_bytes()?;
        EditorMessageBody::ApproveProcessPluginsResponse(message_id, data)
      }
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
    let mut builder = MessageBuilder::new(self.id, self.body.as_u32());
    match &self.body {
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
        let path = body.file_path.to_string_lossy().to_string();
        builder.add_owned_bytes(path.into_bytes());
        builder.add_number(body.range.as_ref().map(|r| r.start as u32).unwrap_or(0));
        builder.add_number(body.range.as_ref().map(|r| r.end as u32).unwrap_or_else(|| body.file_bytes.len() as u32));
        builder.add_bytes(&body.override_config);
        builder.add_bytes(&body.file_bytes);
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
      EditorMessageBody::Capabilities(data) => {
        builder.add_bytes(data);
      }
      EditorMessageBody::CapabilitiesResponse(message_id, data) => {
        builder.add_number(*message_id);
        builder.add_bytes(data);
      }
      EditorMessageBody::ApproveProcessPlugins(data) => {
        builder.add_bytes(data);
      }
      EditorMessageBody::ApproveProcessPluginsResponse(message_id, data) => {
        builder.add_number(*message_id);
        builder.add_bytes(data);
      }
      EditorMessageBody::Unknown(_, _) => unreachable!(), // should never be written
    }
    builder.write(writer)?;
    Ok(())
  }
}

enum NumberOrBytes<'a> {
  Number(u32),
  Bytes(&'a [u8]),
  OwnedBytes(Vec<u8>),
}

struct MessageBuilder<'a> {
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
  /// The capabilities of the editor as JSON (see `EditorCapabilities`), which
  /// an editor sends before any other message. Older editors don't send it.
  Capabilities(Vec<u8>),
  /// The capabilities of the editor service as JSON.
  CapabilitiesResponse(u32, Vec<u8>),
  /// Asks the editor whether to allow running the process plugins of a config
  /// file, which is only sent when its capabilities say it can ask the user.
  /// The data is JSON (see `ApproveProcessPluginsMessage`).
  ApproveProcessPlugins(Vec<u8>),
  /// What the user decided about running the process plugins as JSON (see
  /// `ApproveProcessPluginsResponseMessage`).
  ApproveProcessPluginsResponse(u32, Vec<u8>),
  #[allow(dead_code)]
  Unknown(u32, Vec<u8>),
}

impl EditorMessageBody {
  pub fn as_u32(&self) -> u32 {
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
      EditorMessageBody::Capabilities(_) => 9,
      EditorMessageBody::CapabilitiesResponse(_, _) => 10,
      EditorMessageBody::ApproveProcessPlugins(_) => 11,
      EditorMessageBody::ApproveProcessPluginsResponse(_, _) => 12,
      EditorMessageBody::Unknown(_, _) => unreachable!(),
    }
  }
}

#[derive(Debug)]
pub struct FormatEditorMessageBody {
  pub file_path: PathBuf,
  pub range: FormatRange,
  pub override_config: Vec<u8>,
  pub file_bytes: Vec<u8>,
}

/// What the editor or the editor service supports, which is exchanged with the
/// `Capabilities` message. The editor service only sends a message to the
/// editor that the editor says it supports, so older editors keep working.
///
/// Each capability is an object, which may have more details in the future,
/// and isn't supported when it's not specified.
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorCapabilities {
  /// Whether the `ApproveProcessPlugins` message is supported.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub approve_process_plugins: Option<ApproveProcessPluginsCapability>,
}

#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApproveProcessPluginsCapability {}

/// The data of an `ApproveProcessPlugins` message.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApproveProcessPluginsMessage {
  /// The config file that specifies the process plugins.
  pub config_file: String,
  /// The process plugins.
  pub plugins: Vec<ApproveProcessPluginsPlugin>,
  /// The message to show the user.
  pub message: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApproveProcessPluginsPlugin {
  /// The url or path of the process plugin with its checksum.
  pub source: String,
}

/// The data of an `ApproveProcessPluginsResponse` message.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApproveProcessPluginsResponseMessage {
  pub decision: ProcessPluginsDecision,
}

/// What the user decided in response to an `ApproveProcessPlugins` message.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProcessPluginsDecision {
  /// Run them for this session.
  Allow,
  /// Run them until the configuration changes.
  AlwaysAllow,
  /// Don't run them for this session.
  #[default]
  Deny,
  /// Don't run them or ask again until the configuration changes.
  AlwaysDeny,
  /// A decision this version doesn't know about, which is treated as denying.
  #[serde(other)]
  Unknown,
}

#[cfg(test)]
mod test {
  use pretty_assertions::assert_eq;

  use super::*;

  #[test]
  fn should_deserialize_unknown_data_from_newer_editors() {
    let response: ApproveProcessPluginsResponseMessage = serde_json::from_str(r#"{ "decision": "allowForWorkspace", "other": 1 }"#).unwrap();
    assert_eq!(response.decision, ProcessPluginsDecision::Unknown);
    let capabilities: EditorCapabilities = serde_json::from_str(r#"{ "approveProcessPlugins": { "other": 1 }, "other": {} }"#).unwrap();
    assert_eq!(capabilities.approve_process_plugins, Some(ApproveProcessPluginsCapability {}));
    let capabilities: EditorCapabilities = serde_json::from_str("{}").unwrap();
    assert_eq!(capabilities.approve_process_plugins, None);
  }
}
