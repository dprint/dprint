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
  /// Reads a message of the schema version, which decides what each kind of
  /// message is.
  pub fn read<TRead: Read + Unpin>(reader: &mut MessageReader<TRead>, schema_version: u32) -> Result<EditorMessage> {
    let id = reader.read_u32()?;
    let message_kind = reader.read_u32()?;
    let body_length = reader.read_u32()?;
    let body = match (message_kind, schema_version) {
      (0, _) => EditorMessageBody::Success(reader.read_u32()?),
      (1, _) => {
        let message_id = reader.read_u32()?;
        let data = reader.read_sized_bytes()?;
        EditorMessageBody::Error(message_id, data)
      }
      (2, _) => EditorMessageBody::Close,
      (3, _) => EditorMessageBody::IsAlive,
      (4, 5) => {
        let file_path = reader.read_sized_bytes()?;
        EditorMessageBody::CanFormat(PathBuf::from(String::from_utf8_lossy(&file_path).to_string()))
      }
      (5, 5) => {
        let message_id = reader.read_u32()?;
        let can_format = reader.read_u32()?;
        EditorMessageBody::CanFormatResponse(message_id, can_format)
      }
      (6, 5) => {
        let (file_path, range, override_config, file_bytes) = read_format_body(reader)?;
        EditorMessageBody::Format(FormatEditorMessageBody {
          file_path,
          range,
          file_bytes,
          override_config,
        })
      }
      (7, 5) => {
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
      (8, _) => EditorMessageBody::CancelFormat(reader.read_u32()?),
      (4, 6) => EditorMessageBody::Initialize(reader.read_sized_bytes()?),
      (5, 6) => EditorMessageBody::UpdateSettings(reader.read_sized_bytes()?),
      (6, 6) => {
        let file_path = reader.read_sized_bytes()?;
        let start = reader.read_u32()?;
        let end = reader.read_u32()?;
        let options = reader.read_sized_bytes()?;
        let file_bytes = reader.read_sized_bytes()?;
        EditorMessageBody::FormatFile(FormatFileMessageBody {
          file_path: PathBuf::from(String::from_utf8_lossy(&file_path).to_string()),
          start,
          end,
          options,
          file_bytes,
        })
      }
      (7, 6) => {
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
      (9, 6) => EditorMessageBody::FileInfo(reader.read_sized_bytes()?),
      (10, 6) => {
        let message_id = reader.read_u32()?;
        EditorMessageBody::FileInfoResponse(message_id, reader.read_sized_bytes()?)
      }
      (11, 6) => EditorMessageBody::ConfigInfo(reader.read_sized_bytes()?),
      (12, 6) => {
        let message_id = reader.read_u32()?;
        EditorMessageBody::ConfigInfoResponse(message_id, reader.read_sized_bytes()?)
      }
      (13, 6) => EditorMessageBody::ShowMessage(reader.read_sized_bytes()?),
      (14, 6) => EditorMessageBody::ShowMessageRequest(reader.read_sized_bytes()?),
      (15, 6) => {
        let message_id = reader.read_u32()?;
        EditorMessageBody::ShowMessageRequestResponse(message_id, reader.read_sized_bytes()?)
      }
      (16, 6) => EditorMessageBody::ConfigSchema(reader.read_sized_bytes()?),
      (17, 6) => {
        let message_id = reader.read_u32()?;
        EditorMessageBody::ConfigSchemaResponse(message_id, reader.read_sized_bytes()?)
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
      EditorMessageBody::Initialize(data)
      | EditorMessageBody::UpdateSettings(data)
      | EditorMessageBody::FileInfo(data)
      | EditorMessageBody::ConfigInfo(data)
      | EditorMessageBody::ShowMessage(data)
      | EditorMessageBody::ShowMessageRequest(data)
      | EditorMessageBody::ConfigSchema(data) => {
        builder.add_bytes(data);
      }
      EditorMessageBody::FileInfoResponse(message_id, data)
      | EditorMessageBody::ConfigInfoResponse(message_id, data)
      | EditorMessageBody::ShowMessageRequestResponse(message_id, data)
      | EditorMessageBody::ConfigSchemaResponse(message_id, data) => {
        builder.add_number(*message_id);
        builder.add_bytes(data);
      }
      EditorMessageBody::FormatFile(body) => {
        let path = body.file_path.to_string_lossy().to_string();
        builder.add_owned_bytes(path.into_bytes());
        builder.add_number(body.start);
        builder.add_number(body.end);
        builder.add_bytes(&body.options);
        builder.add_bytes(&body.file_bytes);
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
  /// Initializes the editor service as JSON (see `InitializeMessage`). Schema
  /// version 6.
  Initialize(Vec<u8>),
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
  /// The settings of the editor as JSON (see `EditorSettings`). Schema version 6.
  UpdateSettings(Vec<u8>),
  /// Shows a message to the user as JSON (see `ShowMessageMessage`). Schema
  /// version 6.
  ShowMessage(Vec<u8>),
  /// Shows a message to the user with actions to choose from as JSON (see
  /// `ShowMessageRequestMessage`). Schema version 6.
  ShowMessageRequest(Vec<u8>),
  /// The action the user chose as JSON (see
  /// `ShowMessageRequestResponseMessage`). Schema version 6.
  ShowMessageRequestResponse(u32, Vec<u8>),
  /// Gets the JSON schema of config files as JSON (see `ConfigSchemaMessage`).
  /// Schema version 6.
  ConfigSchema(Vec<u8>),
  /// The JSON schema of config files as JSON (see
  /// `ConfigSchemaResponseMessage`). Schema version 6.
  ConfigSchemaResponse(u32, Vec<u8>),
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
      // schema version 6 reuses the kinds of the schema version 5 messages
      // that it doesn't have
      EditorMessageBody::Initialize(_) => 4,
      EditorMessageBody::UpdateSettings(_) => 5,
      EditorMessageBody::FormatFile(_) => 6,
      EditorMessageBody::FormatFileResponse(_, _) => 7,
      EditorMessageBody::FileInfo(_) => 9,
      EditorMessageBody::FileInfoResponse(_, _) => 10,
      EditorMessageBody::ConfigInfo(_) => 11,
      EditorMessageBody::ConfigInfoResponse(_, _) => 12,
      EditorMessageBody::ShowMessage(_) => 13,
      EditorMessageBody::ShowMessageRequest(_) => 14,
      EditorMessageBody::ShowMessageRequestResponse(_, _) => 15,
      EditorMessageBody::ConfigSchema(_) => 16,
      EditorMessageBody::ConfigSchemaResponse(_, _) => 17,
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
  /// The UTF-16 offset in the file text to start formatting at.
  pub start: u32,
  /// The UTF-16 offset in the file text to end formatting at.
  pub end: u32,
  /// The `FormatFileOptions` as JSON, which may be empty.
  pub options: Vec<u8>,
  pub file_bytes: Vec<u8>,
}

#[derive(Debug)]
pub enum FormatFileResult {
  /// Why the file wasn't formatted as JSON (see `NotFormattedMessage`).
  NotFormatted(Vec<u8>),
  /// The edits that format the file as JSON (see `FormattedMessage`).
  Formatted(Vec<u8>),
}

/// The data of an `Initialize` message.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeMessage {
  pub client_info: ClientInfo,
  #[serde(default)]
  pub settings: EditorSettings,
}

/// The editor that's using the editor service.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
  /// The name of the editor (ex. "vscode").
  pub name: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub version: Option<String>,
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
  /// Whether to allow using the global config file when there's no config file in an
  /// ancestor directory.
  #[serde(default)]
  pub allow_global_config: bool,
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
  /// The message to show the user about why the file can't be formatted
  /// (ex. for a command the user ran).
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub message: Option<String>,
}

/// The data of a `ConfigInfo` message.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigInfoMessage {
  /// A file that the config file is used for (ex. the config file itself).
  pub file_path: PathBuf,
  /// Whether to allow using the global config file when there's no config file in an
  /// ancestor directory.
  #[serde(default)]
  pub allow_global_config: bool,
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
  /// Whether to allow using the global config file when there's no config file in an
  /// ancestor directory.
  #[serde(default)]
  pub allow_global_config: bool,
  /// Whether to format the file again until the output stops changing, which
  /// defaults to the `DPRINT_EDITOR_STABLE_FORMAT` environment variable.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub ensure_stable_format: Option<bool>,
  /// Whether to notify the user when the file isn't formatted because it doesn't
  /// have a config file, which defaults to the setting.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub show_no_config_notification: Option<bool>,
}

/// The edits of a `FormatFile` message that formatted the file.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormattedMessage {
  /// The edits of the text that was sent, which don't overlap and are in
  /// order.
  pub edits: Vec<FormatEdit>,
}

/// Replaces the text between two UTF-16 offsets of the text that was sent.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatEdit {
  pub start: u32,
  pub end: u32,
  pub new_text: String,
}

/// Why a `FormatFile` message didn't format the file.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotFormattedMessage {
  pub reason: NotFormattedReason,
  /// The message to show the user about why the file wasn't formatted (ex. for
  /// a command the user ran), which is `None` when it's already formatted.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub message: Option<String>,
}

/// Why a file isn't formatted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NotFormattedReason {
  /// There's no config file in an ancestor directory and there's no global
  /// config file.
  NoConfigFile,
  /// There's no config file in an ancestor directory and the global config
  /// file isn't allowed to be used.
  GlobalConfigNotAllowed,
  /// The config file doesn't format the file (ex. its includes and excludes
  /// don't match it).
  NotMatched,
  /// No plugin in the config file handles the file.
  NoPlugin,
  /// The config file has no plugins.
  NoPlugins,
  /// The file is already formatted.
  Unchanged,
}

/// The data of an `UpdateSettings` message.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorSettings {
  /// The workspace folders open in the editor.
  #[serde(default)]
  pub workspace_folders: Vec<PathBuf>,
  /// Whether to format the files that don't have a config file in an ancestor
  /// directory with the global config file.
  #[serde(default)]
  pub allow_global_config: bool,
  /// Whether to notify the user when a file isn't formatted because it doesn't
  /// have a config file.
  #[serde(default = "default_true")]
  pub show_no_config_notification: bool,
  /// Whether to log debug information, which defaults to whether the editor
  /// service was started with `--verbose`.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub verbose: Option<bool>,
}

impl Default for EditorSettings {
  fn default() -> Self {
    Self {
      workspace_folders: Vec::new(),
      allow_global_config: false,
      show_no_config_notification: true,
      verbose: None,
    }
  }
}

fn default_true() -> bool {
  true
}

/// The data of a `ShowMessage` message.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShowMessageMessage {
  pub kind: MessageKind,
  pub message: String,
}

/// The data of a `ShowMessageRequest` message.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShowMessageRequestMessage {
  pub kind: MessageKind,
  pub message: String,
  pub actions: Vec<MessageAction>,
}

/// The data of a `ShowMessageRequestResponse` message.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShowMessageRequestResponseMessage {
  /// The action the user chose, which is `None` when they didn't choose one
  /// (ex. they closed the message).
  #[serde(default)]
  pub action: Option<MessageAction>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageAction {
  pub title: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MessageKind {
  Info,
  Warning,
  Error,
}

/// The data of a `ConfigSchema` message.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSchemaMessage {
  /// Files that the config files are used for (ex. the config files
  /// themselves). The schema has the plugins of each config file.
  pub file_paths: Vec<PathBuf>,
  /// Whether to allow using the global config file when there's no config file in an
  /// ancestor directory.
  #[serde(default)]
  pub allow_global_config: bool,
}

/// The data of a `ConfigSchemaResponse` message.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSchemaResponseMessage {
  pub schema: serde_json::Value,
}
