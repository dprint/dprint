mod builder;
mod resolve_config;
mod types;

pub use builder::*;
pub use resolve_config::*;
pub use types::*;

// todo: more tests, but this is currently tested by the javascript code in dprint-plugin-typescript
#[cfg(test)]
mod configuration_tests;
