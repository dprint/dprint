use serde::{Serialize, Deserialize};
use dprint_core::configuration::*;

/// Resolved markdown configuration.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Configuration {
    pub indent_width: u8,
    pub line_width: u32,
    pub use_tabs: bool,
    pub new_line_kind: NewLineKind,
}
