use dprint_core::communication::Message;
use dprint_core::communication::MessageReader;
use dprint_core::communication::MessageWriter;
use std::io::ErrorKind;
use std::io::Read;
use std::io::Result;
use std::io::Write;
use std::path::PathBuf;

use dprint_core::configuration::ConfigKeyMap;
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
      9 => EditorMessageBody::FileInfo(reader.read_sized_bytes()?),
      10 => {
        let message_id = reader.read_u32()?;
        EditorMessageBody::FileInfoResponse(message_id, reader.read_sized_bytes()?)
      }
      11 => EditorMessageBody::ConfigInfo(reader.read_sized_bytes()?),
      12 => {
        let message_id = reader.read_u32()?;
        EditorMessageBody::ConfigInfoResponse(message_id, reader.read_sized_bytes()?)
      }
      13 => {
        let (file_path, range, options, file_bytes) = read_format_body(reader)?;
        EditorMessageBody::FormatFile(FormatFileMessageBody {
          file_path,
          range,
          options,
          file_bytes,
        })
      }
      14 => {
        let message_id = reader.read_u32()?;
        let response_kind = reader.read_u32()?;
        let data = reader.read_sized_bytes()?;
        let result = match response_kind {
          0 => FormatFileResult::NotFormatted(data),
          1 => FormatFileResult::Formatted(data),
          _ => {
            return Err(std::io::Error::new(
              ErrorKind::InvalidData,
              format!("Unknown format file response kind: {}", response_kind),
            ));
          }
        };
        EditorMessageBody::FormatFileResponse(message_id, result)
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
        add_format_body(&mut builder, &body.file_path, &body.range, &body.override_config, &body.file_bytes);
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
      EditorMessageBody::FileInfo(data) | EditorMessageBody::ConfigInfo(data) => {
        builder.add_bytes(data);
      }
      EditorMessageBody::FileInfoResponse(message_id, data) | EditorMessageBody::ConfigInfoResponse(message_id, data) => {
        builder.add_number(*message_id);
        builder.add_bytes(data);
      }
      EditorMessageBody::FormatFile(body) => {
        add_format_body(&mut builder, &body.file_path, &body.range, &body.options, &body.file_bytes);
      }
      EditorMessageBody::FormatFileResponse(message_id, result) => {
        builder.add_number(*message_id);
        match result {
          FormatFileResult::NotFormatted(data) => {
            builder.add_number(0);
            builder.add_bytes(data);
          }
          FormatFileResult::Formatted(data) => {
            builder.add_number(1);
            builder.add_bytes(data);
          }
        }
      }
      EditorMessageBody::Unknown(_, _) => unreachable!(), // should never be written
    }
    builder.write(writer)?;
    Ok(())
  }
}

fn add_format_body<'a>(builder: &mut MessageBuilder<'a>, file_path: &std::path::Path, range: &FormatRange, data: &'a [u8], file_bytes: &'a [u8]) {
  let path = file_path.to_string_lossy().to_string();
  builder.add_owned_bytes(path.into_bytes());
  builder.add_number(range.as_ref().map(|r| r.start as u32).unwrap_or(0));
  builder.add_number(range.as_ref().map(|r| r.end as u32).unwrap_or(file_bytes.len() as u32));
  builder.add_bytes(data);
  builder.add_bytes(file_bytes);
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
  /// Gets information about formatting a file as JSON (see `FileInfoMessage`).
  /// Schema version 6.
  FileInfo(Vec<u8>),
  /// The information about formatting a file as JSON (see
  /// `FileInfoResponseMessage`). Schema version 6.
  FileInfoResponse(u32, Vec<u8>),
  /// Gets information about the config file of a file as JSON (see
  /// `ConfigInfoMessage`). Schema version 6.
  ConfigInfo(Vec<u8>),
  /// The information about the config file of a file as JSON (see
  /// `ConfigInfoResponseMessage`). Schema version 6.
  ConfigInfoResponse(u32, Vec<u8>),
  /// Formats a file with the config file that's found for it. Schema version 6.
  FormatFile(FormatFileMessageBody),
  FormatFileResponse(u32, FormatFileResult),
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
      EditorMessageBody::FileInfo(_) => 9,
      EditorMessageBody::FileInfoResponse(_, _) => 10,
      EditorMessageBody::ConfigInfo(_) => 11,
      EditorMessageBody::ConfigInfoResponse(_, _) => 12,
      EditorMessageBody::FormatFile(_) => 13,
      EditorMessageBody::FormatFileResponse(_, _) => 14,
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

#[derive(Debug)]
pub struct FormatFileMessageBody {
  pub file_path: PathBuf,
  pub range: FormatRange,
  /// The `FormatFileOptions` as JSON, which may be empty.
  pub options: Vec<u8>,
  pub file_bytes: Vec<u8>,
}

#[derive(Debug)]
pub enum FormatFileResult {
  /// Why the file wasn't formatted as JSON (see `NotFormattedMessage`).
  NotFormatted(Vec<u8>),
  /// The formatted file text.
  Formatted(Vec<u8>),
}

/// The data of a `FileInfo` message.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfoMessage {
  pub file_path: PathBuf,
  /// The path of the notebook when the file is one of its cells, which is only
  /// formatted when the notebook would be.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub notebook_path: Option<PathBuf>,
  /// Whether to use the global config file when there's no config file in an
  /// ancestor directory.
  #[serde(default)]
  pub use_global_config: bool,
}

/// The data of a `FileInfoResponse` message.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfoResponseMessage {
  /// The config file that's used for the file, which is `None` when there isn't one.
  pub config_file: Option<ConfigFileInfo>,
  pub can_format: bool,
  /// Why the file can't be formatted.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub reason: Option<NotFormattedReason>,
}

/// The data of a `ConfigInfo` message.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigInfoMessage {
  /// A file that the config file is used for (ex. the config file itself).
  pub file_path: PathBuf,
  /// Whether to use the global config file when there's no config file in an
  /// ancestor directory.
  #[serde(default)]
  pub use_global_config: bool,
}

/// The data of a `ConfigInfoResponse` message.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigInfoResponseMessage {
  /// The config file that's used for the file, which is `None` when there isn't one.
  pub config_file: Option<ConfigFileInfo>,
  /// The plugins of the config file.
  pub plugins: Vec<EditorPluginInfo>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigFileInfo {
  pub path: String,
  /// Whether it's the global config file.
  pub is_global: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorPluginInfo {
  pub name: String,
  pub version: String,
  pub config_key: String,
  pub file_extensions: Vec<String>,
  pub file_names: Vec<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub config_schema_url: Option<String>,
  pub help_url: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub update_url: Option<String>,
}

/// The options of a `FormatFile` message.
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatFileOptions {
  /// The plugin config to use instead of what's in the config file.
  #[serde(default, skip_serializing_if = "ConfigKeyMap::is_empty")]
  pub override_config: ConfigKeyMap,
  /// The path of the notebook when the file is one of its cells, which is only
  /// formatted when the notebook would be.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub notebook_path: Option<PathBuf>,
  /// Whether to use the global config file when there's no config file in an
  /// ancestor directory.
  #[serde(default)]
  pub use_global_config: bool,
}

/// Why a `FormatFile` message didn't format the file.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotFormattedMessage {
  pub reason: NotFormattedReason,
}

/// Why a file isn't formatted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NotFormattedReason {
  /// There's no config file in an ancestor directory and the global config
  /// file isn't used or doesn't exist.
  NoConfigFile,
  /// The config file doesn't format the file (ex. its includes and excludes
  /// don't match it).
  NotMatched,
  /// No plugin in the config file handles the file.
  NoPlugin,
  /// The file is already formatted.
  Unchanged,
}
