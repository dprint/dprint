#![allow(clippy::bool_to_int_with_if)]

#[cfg(feature = "communication")]
pub mod communication;

#[cfg(feature = "formatting")]
pub mod formatting;

pub mod configuration;
#[cfg(any(feature = "process", feature = "wasm"))]
pub mod plugins;

#[cfg(feature = "async_runtime")]
pub mod async_runtime;
