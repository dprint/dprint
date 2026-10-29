//! The messages for why the language server didn't format a document, which
//! are shown for a format command the user ran.

pub const NOT_MATCHED_MESSAGE: &str =
  "dprint did not format this document because the configuration file in use doesn't format it (ex. its \"includes\" and \"excludes\" don't match it).";
pub const NO_PLUGIN_MESSAGE: &str = "dprint did not format this document because no plugin in the configuration file in use handles it.";
pub const DOWNLOAD_REQUIRED_MESSAGE: &str = "dprint did not format this document because the configuration file in use needs plugins from a domain that isn't trusted, which need to be allowed before they're downloaded.";
pub const RANGE_FORMATTING_NOT_SUPPORTED_MESSAGE: &str =
  "dprint did not format the selection because a plugin that formats this document doesn't support formatting part of it.";
pub const PROCESS_PLUGINS_NOT_ALLOWED_MESSAGE: &str = concat!(
  "dprint did not format this document because the process plugins of the configuration file in use weren't allowed to run. ",
  "Allow them when asked or by running `dprint fmt`, `dprint check`, or `dprint config install` in the directory of the configuration file."
);
pub const NO_CELL_PLUGIN_MESSAGE: &str = "dprint did not format this notebook cell. A cell is only formatted when the plugins in the configuration file in use handle both the notebook file (ex. the jupyter plugin) and the cell's language.";
