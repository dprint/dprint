use dprint_core::plugins::PluginInfo;

use crate::types::ErrBox;

#[derive(Clone)]
pub struct CompilationResult {
    pub bytes: Vec<u8>,
    pub plugin_info: PluginInfo,
}

// trait alias hack (https://www.worthe-it.co.za/programming/2017/01/15/aliasing-traits-in-rust.html)
pub trait CompileFn: Fn(&[u8]) -> Result<CompilationResult, ErrBox> + std::marker::Sync + 'static + Clone {
}

impl<T> CompileFn for T where T : Fn(&[u8]) -> Result<CompilationResult, ErrBox> + std::marker::Sync + 'static + Clone {
}
