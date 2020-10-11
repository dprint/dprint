use std::path::Path;
use std::borrow::Cow;

/// The process plugin schema version.
pub const PLUGIN_SCHEMA_VERSION: u32 = 3;

/// Kinds of messages that process plugins must handle.
#[derive(Debug)]
pub enum MessageKind {
    GetPluginSchemaVersion = 0,
    GetPluginInfo = 1,
    GetLicenseText = 2,
    GetResolvedConfig = 3,
    SetGlobalConfig = 4,
    SetPluginConfig = 5,
    GetConfigDiagnostics = 6,
    /// Returns a format result part, then a file text part.
    FormatText = 7,
    Close = 8,
}

// todo: generate with a macro
impl From<u32> for MessageKind {
    fn from(kind: u32) -> Self {
        match kind {
            0 => MessageKind::GetPluginSchemaVersion,
            1 => MessageKind::GetPluginInfo,
            2 => MessageKind::GetLicenseText,
            3 => MessageKind::GetResolvedConfig,
            4 => MessageKind::SetGlobalConfig,
            5 => MessageKind::SetPluginConfig,
            6 => MessageKind::GetConfigDiagnostics,
            7 => MessageKind::FormatText,
            8 => MessageKind::Close,
            _ => unreachable!("Unexpected message kind: {}", kind),
        }
    }
}

/// The kinds of responses.
#[derive(Debug)]
pub enum ResponseKind {
    Success = 0,
    Error = 1,
}

// todo: generate with a macro
impl From<u32> for ResponseKind {
    fn from(orig: u32) -> Self {
        match orig {
            0 => ResponseKind::Success,
            1 => ResponseKind::Error,
            _ => unreachable!("Unexpected response kind: {}", orig),
        }
    }
}

/// The kinds of format results.
#[derive(Debug)]
pub enum FormatResult {
    NoChange = 0,
    Change = 1,
    RequestTextFormat = 2,
}

// todo: generate with a macro
impl From<u32> for FormatResult {
    fn from(orig: u32) -> Self {
        match orig {
            0 => FormatResult::NoChange,
            1 => FormatResult::Change,
            2 => FormatResult::RequestTextFormat,
            _ => unreachable!("Unexpected format result: {}", orig),
        }
    }
}

/// The kinds of host format results.
#[derive(Debug)]
pub enum HostFormatResult {
    NoChange = 0,
    Change = 1,
    Error = 2,
}

// todo: generate with a macro
impl From<u32> for HostFormatResult {
    fn from(orig: u32) -> Self {
        match orig {
            0 => HostFormatResult::NoChange,
            1 => HostFormatResult::Change,
            2 => HostFormatResult::Error,
            _ => unreachable!("Unexpected host format result: {}", orig),
        }
    }
}

pub enum MessagePart<'a> {
    VariableData(Cow<'a, [u8]>),
    Number(u32)
}

impl<'a> From<&'a Path> for MessagePart<'a> {
    fn from(value: &'a Path) -> Self {
        match value.to_string_lossy() {
            Cow::Owned(value) => value.into(),
            Cow::Borrowed(value) => value.into(),
        }
    }
}

impl<'a> From<String> for MessagePart<'a> {
    fn from(value: String) -> Self {
        MessagePart::VariableData(Cow::Owned(value.into_bytes()))
    }
}

impl<'a> From<&'a str> for MessagePart<'a> {
    fn from(value: &'a str) -> Self {
        MessagePart::VariableData(Cow::Borrowed(value.as_bytes()))
    }
}

impl<'a> From<Cow<'a, str>> for MessagePart<'a> {
    fn from(value: Cow<'a, str>) -> Self {
        match value {
            Cow::Owned(value) => value.into(),
            Cow::Borrowed(value) => value.into(),
        }
    }
}

impl<'a> From<&'a [u8]> for MessagePart<'a> {
    fn from(value: &'a [u8]) -> Self {
        MessagePart::VariableData(Cow::Borrowed(value))
    }
}

impl<'a> From<&'a Vec<u8>> for MessagePart<'a> {
    fn from(value: &'a Vec<u8>) -> Self {
        MessagePart::VariableData(Cow::Borrowed(value))
    }
}

impl<'a> From<Vec<u8>> for MessagePart<'a> {
    fn from(value: Vec<u8>) -> Self {
        MessagePart::VariableData(Cow::Owned(value))
    }
}

impl<'a> From<u32> for MessagePart<'a> {
    fn from(value: u32) -> Self {
        MessagePart::Number(value)
    }
}
