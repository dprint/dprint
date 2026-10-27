//! The messages of schema version 6 of the editor service.

use dprint_core::communication::MessageReader;
use std::io::ErrorKind;
use std::io::Read;
use std::io::Result;
use std::path::PathBuf;

use dprint_core::configuration::ConfigKeyMap;

use super::super::messages::EditorPluginInfo;
use super::super::messages::MessageBody;
use super::super::messages::MessageBuilder;

pub type EditorMessage = super::super::messages::EditorMessage<EditorMessageBody>;

#[derive(Debug)]
pub enum EditorMessageBody {
  Success(u32),
  Error(u32, Vec<u8>),
  Close,
  IsAlive,
  /// Initializes the editor service as JSON (see `InitializeMessage`).
  Initialize(Vec<u8>),
  /// The settings of the editor as JSON (see `EditorSettings`).
  UpdateSettings(Vec<u8>),
  /// Formats a file with the config file that's found for it.
  FormatFile(FormatFileMessageBody),
  FormatFileResponse(u32, FormatFileResult),
  CancelFormat(u32),
  /// Gets information about formatting a file as JSON (see `FileInfoMessage`).
  FileInfo(Vec<u8>),
  /// The information about formatting a file as JSON (see
  /// `FileInfoResponseMessage`).
  FileInfoResponse(u32, Vec<u8>),
  /// Gets information about the config file of a file as JSON (see
  /// `ConfigInfoMessage`).
  ConfigInfo(Vec<u8>),
  /// The information about the config file of a file as JSON (see
  /// `ConfigInfoResponseMessage`).
  ConfigInfoResponse(u32, Vec<u8>),
  /// Shows a message to the user as JSON (see `ShowMessageMessage`).
  ShowMessage(Vec<u8>),
  /// Shows a message to the user with actions to choose from as JSON (see
  /// `ShowMessageRequestMessage`).
  ShowMessageRequest(Vec<u8>),
  /// The action the user chose as JSON (see
  /// `ShowMessageRequestResponseMessage`).
  ShowMessageRequestResponse(u32, Vec<u8>),
  /// Gets the JSON schema of config files as JSON (see `ConfigSchemaMessage`).
  ConfigSchema(Vec<u8>),
  /// The JSON schema of config files as JSON (see
  /// `ConfigSchemaResponseMessage`).
  ConfigSchemaResponse(u32, Vec<u8>),
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
      4 => EditorMessageBody::Initialize(reader.read_sized_bytes()?),
      5 => EditorMessageBody::UpdateSettings(reader.read_sized_bytes()?),
      6 => {
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
      7 => {
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
      13 => EditorMessageBody::ShowMessage(reader.read_sized_bytes()?),
      14 => EditorMessageBody::ShowMessageRequest(reader.read_sized_bytes()?),
      15 => {
        let message_id = reader.read_u32()?;
        EditorMessageBody::ShowMessageRequestResponse(message_id, reader.read_sized_bytes()?)
      }
      16 => EditorMessageBody::ConfigSchema(reader.read_sized_bytes()?),
      17 => {
        let message_id = reader.read_u32()?;
        EditorMessageBody::ConfigSchemaResponse(message_id, reader.read_sized_bytes()?)
      }
      _ => {
        let data = reader.read_bytes(body_length as usize)?;
        EditorMessageBody::Unknown(kind, data)
      }
    })
  }

  fn write<'a>(&'a self, builder: &mut MessageBuilder<'a>) {
    match self {
      EditorMessageBody::Success(message_id) | EditorMessageBody::CancelFormat(message_id) => {
        builder.add_number(*message_id);
      }
      EditorMessageBody::Close | EditorMessageBody::IsAlive => {}
      EditorMessageBody::Initialize(data)
      | EditorMessageBody::UpdateSettings(data)
      | EditorMessageBody::FileInfo(data)
      | EditorMessageBody::ConfigInfo(data)
      | EditorMessageBody::ShowMessage(data)
      | EditorMessageBody::ShowMessageRequest(data)
      | EditorMessageBody::ConfigSchema(data) => {
        builder.add_bytes(data);
      }
      EditorMessageBody::Error(message_id, data)
      | EditorMessageBody::FileInfoResponse(message_id, data)
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
  }

  fn kind(&self) -> u32 {
    match self {
      EditorMessageBody::Success(_) => 0,
      EditorMessageBody::Error(_, _) => 1,
      EditorMessageBody::Close => 2,
      EditorMessageBody::IsAlive => 3,
      EditorMessageBody::Initialize(_) => 4,
      EditorMessageBody::UpdateSettings(_) => 5,
      EditorMessageBody::FormatFile(_) => 6,
      EditorMessageBody::FormatFileResponse(_, _) => 7,
      EditorMessageBody::CancelFormat(_) => 8,
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

  fn error(message_id: u32, data: Vec<u8>) -> Self {
    EditorMessageBody::Error(message_id, data)
  }
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
  /// The problems with the configuration, which are only for `ConfigDiagnostics`.
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub diagnostics: Vec<ConfigDiagnostic>,
}

/// A problem with the configuration of the config file.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigDiagnostic {
  /// The plugin whose configuration has the problem, which is `None` for a
  /// problem outside a plugin's configuration.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub plugin_name: Option<String>,
  pub property_name: String,
  pub message: String,
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
  /// The configuration of a plugin that formats the file has problems (see
  /// `diagnostics`).
  ConfigDiagnostics,
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
