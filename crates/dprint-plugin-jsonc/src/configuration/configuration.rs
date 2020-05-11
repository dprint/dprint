use dprint_core::configuration::NewLineKind;
use serde::{Serialize, Deserialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Configuration {
    pub line_width: u32,
    pub use_tabs: bool,
    pub indent_width: u8,
    pub new_line_kind: NewLineKind,
    #[serde(rename = "commentLine.forceSpaceAfterSlashes")]
    pub comment_line_force_space_after_slashes: bool,
}
