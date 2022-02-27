/// The process plugin schema version.
pub const PLUGIN_SCHEMA_VERSION: u32 = 4;

/// Kinds of messages that process plugins must handle.
#[derive(Debug)]
pub enum MessageKind {
  Close = 1,
  GetPluginInfo = 2,
  GetLicenseText = 3,
  RegisterConfig = 4,
  ReleaseConfig = 5,
  GetConfigDiagnostics = 6,
  GetResolvedConfig = 7,
  FormatText = 8,
  CancelFormat = 9,
  HostFormatResponse = 10,
}

// todo: generate with a macro
impl From<u32> for MessageKind {
  fn from(kind: u32) -> Self {
    use MessageKind::*;
    match kind {
      1 => Close,
      2 => GetPluginInfo,
      3 => GetLicenseText,
      4 => RegisterConfig,
      5 => ReleaseConfig,
      6 => GetConfigDiagnostics,
      7 => GetResolvedConfig,
      8 => FormatText,
      9 => CancelFormat,
      10 => HostFormatResponse,
      _ => unreachable!("Unexpected message kind: {}", kind),
    }
  }
}

/// The kinds of responses.
#[derive(Debug)]
pub enum ResponseKind {
  Success = 0,
  Error = 1,
  HostFormatRequest = 2,
}

// todo: generate with a macro
impl From<u32> for ResponseKind {
  fn from(orig: u32) -> Self {
    match orig {
      0 => ResponseKind::Success,
      1 => ResponseKind::Error,
      2 => ResponseKind::HostFormatRequest,
      _ => unreachable!("Unexpected response kind: {}", orig),
    }
  }
}

/// The kinds of format results.
#[derive(Debug)]
pub enum FormatResult {
  NoChange = 0,
  Change = 1,
}

// todo: generate with a macro
impl From<u32> for FormatResult {
  fn from(orig: u32) -> Self {
    match orig {
      0 => FormatResult::NoChange,
      1 => FormatResult::Change,
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
      _ => unreachable!("Unexpected host format result: {}", orig),
    }
  }
}
