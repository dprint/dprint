extern crate pulldown_cmark;
extern crate dprint_core;

pub mod configuration;
mod format_text;
mod parsing;

pub use format_text::{format_text};
