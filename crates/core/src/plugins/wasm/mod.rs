/// The plugin system schema version that is incremented
/// when there are any breaking changes.
pub const PLUGIN_SYSTEM_SCHEMA_VERSION: u32 = 3;

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
        file_text: String,
        override_config: &dprint_core::configuration::ConfigKeyMap,
      ) -> anyhow::Result<String> {
        #[link(wasm_import_module = "dprint")]
        extern "C" {
          fn host_clear_bytes(length: u32);
          fn host_read_buffer(pointer: u32, length: u32);
          fn host_write_buffer(pointer: u32, offset: u32, length: u32);
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
        send_string_to_host(file_text.clone());

        return match unsafe { host_format() } {
          0 => {
            // no change
            Ok(file_text)
          }
          1 => {
            // change
            let length = unsafe { host_get_formatted_text() };
            let formatted_text = get_string_from_host(length);
            Ok(formatted_text)
          }
          2 => {
            // error
            let length = unsafe { host_get_error_text() };
            let error_text = get_string_from_host(length);
            Err(dprint_core::types::Error::new(error_text))
          }
          _ => unreachable!(),
        };

        fn send_string_to_host(text: String) {
          let mut index = 0;
          let length = set_shared_bytes_str(text);
          unsafe {
            host_clear_bytes(length as u32);
          }
          while index < length {
            let read_count = std::cmp::min(length - index, WASM_MEMORY_BUFFER_SIZE);
            set_buffer_with_shared_bytes(index, read_count);
            unsafe {
              host_read_buffer(get_wasm_memory_buffer() as u32, read_count as u32);
            }
            index += read_count;
          }
        }

        fn get_string_from_host(length: u32) -> String {
          let mut index: u32 = 0;
          clear_shared_bytes(length as usize);
          while index < length {
            let read_count = std::cmp::min(length - index, WASM_MEMORY_BUFFER_SIZE as u32);
            unsafe {
              host_write_buffer(get_wasm_memory_buffer() as u32, index, read_count);
            }
            add_to_shared_bytes_from_buffer(read_count as usize);
            index += read_count;
          }
          take_string_from_shared_bytes()
        }
      }

      // FORMATTING

      static OVERRIDE_CONFIG: StaticCell<Option<dprint_core::configuration::ConfigKeyMap>> = StaticCell::new(None);
      static FILE_PATH: StaticCell<Option<std::path::PathBuf>> = StaticCell::new(None);
      static FORMATTED_TEXT: StaticCell<Option<String>> = StaticCell::new(None);
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
        let file_text = take_string_from_shared_bytes();

        let formatted_text = unsafe { WASM_PLUGIN.get().format_text(&file_path, &file_text, &config, format_with_host) };
        match formatted_text {
          Ok(formatted_text) => {
            if formatted_text == file_text {
              0 // no change
            } else {
              unsafe { FORMATTED_TEXT.get().replace(formatted_text) };
              1 // change
            }
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
        set_shared_bytes_str(formatted_text)
      }

      #[no_mangle]
      pub fn get_error_text() -> usize {
        let error_text = unsafe { ERROR_TEXT.get().take().expect("Expected to have error text.") };
        set_shared_bytes_str(error_text)
      }

      // INFORMATION & CONFIGURATION

      static RESOLVE_CONFIGURATION_RESULT: StaticCell<Option<dprint_core::configuration::ResolveConfigurationResult<Configuration>>> = StaticCell::new(None);

      #[no_mangle]
      pub fn get_plugin_info() -> usize {
        use dprint_core::plugins::PluginInfo;
        let plugin_info = unsafe { WASM_PLUGIN.get().get_plugin_info() };
        let info_json = serde_json::to_string(&plugin_info).unwrap();
        set_shared_bytes_str(info_json)
      }

      #[no_mangle]
      pub fn get_license_text() -> usize {
        set_shared_bytes_str(unsafe { WASM_PLUGIN.get().get_license_text() })
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

      fn get_resolved_config_result<'a>() -> &'a dprint_core::configuration::ResolveConfigurationResult<Configuration> {
        unsafe {
          ensure_initialized();
          return RESOLVE_CONFIGURATION_RESULT.get().as_ref().unwrap();
        }
      }

      fn ensure_initialized() {
        unsafe {
          if RESOLVE_CONFIGURATION_RESULT.get().is_none() {
            let config_result = create_resolved_config_result(std::collections::HashMap::new());
            RESOLVE_CONFIGURATION_RESULT.get().replace(config_result);
          }
        }
      }

      fn create_resolved_config_result(
        override_config: dprint_core::configuration::ConfigKeyMap,
      ) -> dprint_core::configuration::ResolveConfigurationResult<Configuration> {
        unsafe {
          if let Some(global_config) = GLOBAL_CONFIG.get().as_ref() {
            if let Some(plugin_config) = PLUGIN_CONFIG.get().as_ref() {
              let mut plugin_config = plugin_config.clone();
              for (key, value) in override_config {
                plugin_config.insert(key, value);
              }
              return unsafe { WASM_PLUGIN.get().resolve_config(plugin_config, global_config) };
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

      const WASM_MEMORY_BUFFER_SIZE: usize = 4 * 1024;
      static mut WASM_MEMORY_BUFFER: [u8; WASM_MEMORY_BUFFER_SIZE] = [0; WASM_MEMORY_BUFFER_SIZE];
      static SHARED_BYTES: StaticCell<Vec<u8>> = StaticCell::new(Vec::new());

      #[no_mangle]
      pub fn get_plugin_schema_version() -> u32 {
        dprint_core::plugins::wasm::PLUGIN_SYSTEM_SCHEMA_VERSION
      }

      #[no_mangle]
      pub fn get_wasm_memory_buffer() -> *const u8 {
        unsafe { WASM_MEMORY_BUFFER.as_ptr() }
      }

      #[no_mangle]
      pub fn get_wasm_memory_buffer_size() -> usize {
        WASM_MEMORY_BUFFER_SIZE
      }

      #[no_mangle]
      pub fn add_to_shared_bytes_from_buffer(length: usize) {
        unsafe { SHARED_BYTES.get().extend(&WASM_MEMORY_BUFFER[..length]) }
      }

      #[no_mangle]
      pub fn set_buffer_with_shared_bytes(offset: usize, length: usize) {
        unsafe {
          let bytes = &SHARED_BYTES.get()[offset..(offset + length)];
          &WASM_MEMORY_BUFFER[..length].copy_from_slice(bytes);
        }
      }

      #[no_mangle]
      pub fn clear_shared_bytes(capacity: usize) {
        SHARED_BYTES.replace(Vec::with_capacity(capacity));
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
