/// The plugin system schema version that is incremented
/// when there are any breaking changes.
pub const PLUGIN_SYSTEM_SCHEMA_VERSION: u32 = 4;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub mod macros {
  #[macro_export]
  macro_rules! generate_plugin_code {
    ($wasm_plugin_struct:ident, $wasm_plugin_creation:expr) => {
      // This is ok to do because Wasm plugins are only ever executed on a single thread.
      // https://github.com/rust-lang/rust/issues/53639#issuecomment-790091647
      struct StaticCell<T>(std::cell::UnsafeCell<T>);

      impl<T> StaticCell<T> {
        const fn new(value: T) -> Self {
          StaticCell(std::cell::UnsafeCell::new(value))
        }

        #[allow(clippy::mut_from_ref)]
        unsafe fn get(&self) -> &mut T {
          &mut *self.0.get()
        }

        fn replace(&self, value: T) -> T {
          std::mem::replace(unsafe { self.get() }, value)
        }
      }

      unsafe impl<T> Sync for StaticCell<T> {}

      static WASM_PLUGIN: StaticCell<$wasm_plugin_struct> = StaticCell::new($wasm_plugin_creation);

      // HOST FORMATTING

      fn format_with_host(
        file_path: &std::path::Path,
        file_bytes: Vec<u8>,
        override_config: &dprint_core::configuration::ConfigKeyMap,
      ) -> anyhow::Result<Option<Vec<u8>>> {
        #[link(wasm_import_module = "dprint")]
        extern "C" {
          fn host_read_buffer(pointer: u32, length: u32);
          fn host_write_buffer(pointer: u32);
          fn host_take_file_path();
          fn host_take_override_config();
          fn host_format() -> u8;
          fn host_get_formatted_text() -> u32;
          fn host_get_error_text() -> u32;
        }

        if !override_config.is_empty() {
          send_string_to_host(serde_json::to_string(override_config).unwrap());
          unsafe {
            host_take_override_config();
          }
        }

        send_string_to_host(file_path.to_string_lossy().to_string());
        unsafe {
          host_take_file_path();
        }
        send_bytes_to_host(file_bytes);

        return match unsafe { host_format() } {
          0 => {
            // no change
            Ok(None)
          }
          1 => {
            // change
            let length = unsafe { host_get_formatted_text() };
            let formatted_text = get_bytes_from_host(length);
            Ok(Some(formatted_text))
          }
          2 => {
            // error
            let length = unsafe { host_get_error_text() };
            let error_text = get_string_from_host(length);
            Err(anyhow::anyhow!("{}", error_text))
          }
          value => panic!("unknown host format value: {}", value),
        };

        fn send_string_to_host(text: String) {
          send_bytes_to_host(text.into_bytes())
        }

        fn send_bytes_to_host(bytes: Vec<u8>) {
          let mut index = 0;
          let length = set_shared_bytes(bytes);
          unsafe {
            host_read_buffer(get_shared_bytes_buffer() as u32, length as u32);
          }
        }

        fn get_string_from_host(length: u32) -> String {
          String::from_utf8(get_bytes_from_host(length)).unwrap()
        }

        fn get_bytes_from_host(length: u32) -> Vec<u8> {
          let mut index: u32 = 0;
          clear_shared_bytes(length as usize);
          unsafe {
            host_write_buffer(get_shared_bytes_buffer() as u32);
          }
          take_from_shared_bytes()
        }
      }

      // FORMATTING

      static OVERRIDE_CONFIG: StaticCell<Option<dprint_core::configuration::ConfigKeyMap>> = StaticCell::new(None);
      static FILE_PATH: StaticCell<Option<std::path::PathBuf>> = StaticCell::new(None);
      static FORMATTED_TEXT: StaticCell<Option<Vec<u8>>> = StaticCell::new(None);
      static ERROR_TEXT: StaticCell<Option<String>> = StaticCell::new(None);

      #[no_mangle]
      pub fn set_override_config() {
        let bytes = take_from_shared_bytes();
        let config = serde_json::from_slice(&bytes).unwrap();
        unsafe { OVERRIDE_CONFIG.get().replace(config) };
      }

      #[no_mangle]
      pub fn set_file_path() {
        // convert windows back slashes to forward slashes so it works with PathBuf
        let text = take_string_from_shared_bytes().replace("\\", "/");
        unsafe { FILE_PATH.get().replace(std::path::PathBuf::from(text)) };
      }

      #[no_mangle]
      pub fn format() -> u8 {
        ensure_initialized();
        let config = unsafe {
          if let Some(override_config) = OVERRIDE_CONFIG.get().take() {
            std::borrow::Cow::Owned(create_resolved_config_result(override_config).config)
          } else {
            std::borrow::Cow::Borrowed(&get_resolved_config_result().config)
          }
        };
        let file_path = unsafe { FILE_PATH.get().take().expect("Expected the file path to be set.") };
        let file_bytes = take_from_shared_bytes();

        let formatted_text = unsafe { WASM_PLUGIN.get().format(&file_path, file_bytes, &config, format_with_host) };
        match formatted_text {
          Ok(None) => {
            0 // no change
          }
          Ok(Some(formatted_text)) => {
            unsafe { FORMATTED_TEXT.get().replace(formatted_text) };
            1 // change
          }
          Err(err_text) => {
            unsafe { ERROR_TEXT.get().replace(err_text.to_string()) };
            2 // error
          }
        }
      }

      #[no_mangle]
      pub fn get_formatted_text() -> usize {
        let formatted_text = unsafe { FORMATTED_TEXT.get().take().expect("Expected to have formatted text.") };
        set_shared_bytes(formatted_text)
      }

      #[no_mangle]
      pub fn get_error_text() -> usize {
        let error_text = unsafe { ERROR_TEXT.get().take().expect("Expected to have error text.") };
        set_shared_bytes_str(error_text)
      }

      // INFORMATION & CONFIGURATION

      static RESOLVE_CONFIGURATION_RESULT: StaticCell<Option<dprint_core::plugins::PluginResolveConfigurationResult<Configuration>>> = StaticCell::new(None);

      #[no_mangle]
      pub fn get_plugin_info() -> usize {
        use dprint_core::plugins::PluginInfo;
        let plugin_info = unsafe { WASM_PLUGIN.get().plugin_info() };
        let info_json = serde_json::to_string(&plugin_info).unwrap();
        set_shared_bytes_str(info_json)
      }

      #[no_mangle]
      pub fn get_license_text() -> usize {
        set_shared_bytes_str(unsafe { WASM_PLUGIN.get().license_text() })
      }

      #[no_mangle]
      pub fn get_resolved_config() -> usize {
        let bytes = serde_json::to_vec(&get_resolved_config_result().config).unwrap();
        set_shared_bytes(bytes)
      }

      #[no_mangle]
      pub fn get_config_diagnostics() -> usize {
        let bytes = serde_json::to_vec(&get_resolved_config_result().diagnostics).unwrap();
        set_shared_bytes(bytes)
      }

      // for clearing the config in the playground
      #[no_mangle]
      pub fn reset_config() {
        unsafe {
          RESOLVE_CONFIGURATION_RESULT.get().take();
        }
      }

      fn get_resolved_config_result<'a>() -> &'a dprint_core::plugins::PluginResolveConfigurationResult<Configuration> {
        unsafe {
          ensure_initialized();
          return RESOLVE_CONFIGURATION_RESULT.get().as_ref().unwrap();
        }
      }

      fn ensure_initialized() {
        unsafe {
          if RESOLVE_CONFIGURATION_RESULT.get().is_none() {
            let config_result = create_resolved_config_result(dprint_core::configuration::ConfigKeyMap::new());
            RESOLVE_CONFIGURATION_RESULT.get().replace(config_result);
          }
        }
      }

      fn create_resolved_config_result(
        override_config: dprint_core::configuration::ConfigKeyMap,
      ) -> dprint_core::plugins::PluginResolveConfigurationResult<Configuration> {
        unsafe {
          if let Some(global_config) = GLOBAL_CONFIG.get().as_ref() {
            if let Some(plugin_config) = PLUGIN_CONFIG.get().as_ref() {
              let mut plugin_config = plugin_config.clone();
              for (key, value) in override_config {
                plugin_config.insert(key, value);
              }
              return WASM_PLUGIN.get().resolve_config(plugin_config, global_config);
            }
          }
        }

        panic!("Plugin must have global config and plugin config set before use.");
      }

      // INITIALIZATION

      static GLOBAL_CONFIG: StaticCell<Option<dprint_core::configuration::GlobalConfiguration>> = StaticCell::new(None);
      static PLUGIN_CONFIG: StaticCell<Option<dprint_core::configuration::ConfigKeyMap>> = StaticCell::new(None);

      #[no_mangle]
      pub fn set_global_config() {
        let bytes = take_from_shared_bytes();
        let global_config: dprint_core::configuration::GlobalConfiguration = serde_json::from_slice(&bytes).unwrap();
        unsafe {
          GLOBAL_CONFIG.get().replace(global_config);
          RESOLVE_CONFIGURATION_RESULT.get().take(); // clear
        }
      }

      #[no_mangle]
      pub fn set_plugin_config() {
        let bytes = take_from_shared_bytes();
        let plugin_config: dprint_core::configuration::ConfigKeyMap = serde_json::from_slice(&bytes).unwrap();
        unsafe {
          PLUGIN_CONFIG.get().replace(plugin_config);
          RESOLVE_CONFIGURATION_RESULT.get().take(); // clear
        }
      }

      // LOW LEVEL SENDING AND RECEIVING

      static SHARED_BYTES: StaticCell<Vec<u8>> = StaticCell::new(Vec::new());

      #[no_mangle]
      pub fn dprint_plugin_version_4() -> u32 {
        dprint_core::plugins::wasm::PLUGIN_SYSTEM_SCHEMA_VERSION
      }

      #[no_mangle]
      pub fn get_shared_bytes_buffer() -> *const u8 {
        unsafe { SHARED_BYTES.get().as_ptr() }
      }

      #[no_mangle]
      pub fn clear_shared_bytes(size: usize) {
        SHARED_BYTES.replace(vec![0; size]);
      }

      fn take_string_from_shared_bytes() -> String {
        String::from_utf8(take_from_shared_bytes()).unwrap()
      }

      fn take_from_shared_bytes() -> Vec<u8> {
        SHARED_BYTES.replace(Vec::new())
      }

      fn set_shared_bytes_str(text: String) -> usize {
        set_shared_bytes(text.into_bytes())
      }

      fn set_shared_bytes(bytes: Vec<u8>) -> usize {
        let length = bytes.len();
        SHARED_BYTES.replace(bytes);
        length
      }
    };
  }
}
