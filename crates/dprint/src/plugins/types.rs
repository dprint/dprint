use dprint_core::plugins::PluginInfo;

#[derive(Clone)]
pub struct CompilationResult {
    pub bytes: Vec<u8>,
    pub plugin_info: PluginInfo,
}
