mod compile;
mod instance;
mod load_instance;
mod plugin;
mod resolution_cache;
mod setup_wasm_plugin;

pub use compile::*;
use instance::*;
pub use load_instance::WASM_PLUGIN_THREAD_STACK_SIZE;
pub use load_instance::WasmModuleCreator;
pub use load_instance::precompile_compatibility_hash;
use load_instance::*;
pub use plugin::*;
pub use resolution_cache::*;
pub use setup_wasm_plugin::*;
