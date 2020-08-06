/// The process plugin schema version.
pub const PLUGIN_SCHEMA_VERSION: u32 = 2;

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
