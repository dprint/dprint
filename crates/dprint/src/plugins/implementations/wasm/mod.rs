mod compile;
mod functions;
mod import_object;
mod load_instance;
mod plugin;
mod setup_wasm_plugin;

pub use compile::*;
use functions::*;
pub use import_object::*;
pub use load_instance::WasmModuleCreator;
use load_instance::*;
pub use plugin::*;
pub use setup_wasm_plugin::*;
