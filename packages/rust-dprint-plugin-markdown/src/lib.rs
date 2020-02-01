extern crate comrak;

pub mod configuration;
mod format_text;

pub use format_text::{format_text};

#[cfg(test)]
mod configuration_tests;
