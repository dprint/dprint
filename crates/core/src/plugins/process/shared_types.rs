/// The newest process plugin schema version, which the host and the plugin
/// agree on in the schema establishment phase.
pub const PLUGIN_SCHEMA_VERSION: u32 = 6;

/// The oldest process plugin schema version that's supported, which is the one
/// from before the schema version was negotiated.
pub const MIN_PLUGIN_SCHEMA_VERSION: u32 = 5;

/// The schema version that added the format ranges message. A plugin from
/// before it is given one range at a time.
pub const FORMAT_RANGES_SCHEMA_VERSION: u32 = 6;

/// The environment variable the host sets on the plugin's process with the
/// newest schema version it supports. The plugin responds with the newest one
/// they both support, and with `MIN_PLUGIN_SCHEMA_VERSION` when it isn't set
/// because a host from before then only accepts that version.
pub const MAX_SCHEMA_VERSION_ENV_VAR: &str = "DPRINT_MAX_SCHEMA_VERSION";
