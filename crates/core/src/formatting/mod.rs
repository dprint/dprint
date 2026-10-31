//! Building blocks for writing a code formatter: generate print items (IR) and print them.
//!
//! See [the overview](https://github.com/dprint/dprint/blob/main/docs/overview.md) for how it works and an example.

pub mod actions;
pub mod condition_helpers;
pub mod condition_resolvers;
pub mod conditions;
pub mod ir_helpers;

mod arena;
mod collections;
mod infinite_reevaluation_protection;
mod print;
mod print_items;
mod printer;
mod thread_state;
#[cfg(feature = "tracing")]
mod tracing;
mod write_items;
mod writer;

pub mod tokens;
pub mod utils;

pub use arena::Arena;
pub use arena::ArenaPool;
/// The bump allocator crate used by [`Arena`], for allocating other values in it
/// (ex. `bumpalo::collections::Vec::new_in(arena.bump())`).
pub use bumpalo;
pub use print::PrintOptions;
#[cfg(feature = "tracing")]
pub use print::TracingResult;
pub use print::format;
pub use print::print;
#[cfg(feature = "tracing")]
pub use print::trace_printing;
pub use print_items::*;
use printer::*;
#[cfg(feature = "tracing")]
use tracing::*;
use write_items::*;
