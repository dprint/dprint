/// The plugin system schema version that is incremented
/// when there are any breaking changes.
pub const PLUGIN_SYSTEM_SCHEMA_VERSION: u32 = 4;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
extern "C" {
  fn fd_write(fd: i32, iovs: *const crate::plugins::wasm::Iovec, iovs_len: i32, nwritten: *mut i32) -> i32;
}

pub struct WasiPrintFd(pub i32);

impl std::io::Write for WasiPrintFd {
  fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
      let iovec = Iovec {
        buf: buf.as_ptr(),
        buf_len: buf.len(),
      };
      let mut nwritten: i32 = 0;
      let result = unsafe { fd_write(self.0, &iovec, 1, &mut nwritten) };
      if result == 0 {
        Ok(nwritten as usize)
      } else {
        Err(std::io::Error::from_raw_os_error(result))
      }
    }
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    {
      let size = buf.len();
      match self.0 {
        0 => std::io::stdout().write_all(buf)?,
        1 => std::io::stderr().write_all(buf)?,
        _ => return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput)),
      }
      Ok(size)
    }
  }

  fn flush(&mut self) -> std::io::Result<()> {
    Ok(())
  }
}

#[repr(C)]
pub struct Iovec {
  pub buf: *const u8,
  pub buf_len: usize,
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub mod macros {
  #[macro_export]
  macro_rules! generate_plugin_code {
    ($wasm_plugin_struct:ident, $wasm_plugin_creation:expr) => {
      struct RefStaticCell<T: Default>(std::cell::OnceCell<StaticCell<T>>);

      impl<T: Default> RefStaticCell<T> {
        pub const fn new() -> Self {
          RefStaticCell(std::cell::OnceCell::new())
        }

        #[allow(clippy::mut_from_ref)]
        unsafe fn get(&self) -> &mut T {
          let inner = self.0.get_or_init(Default::default);
          inner.get()
        }

        fn replace(&self, value: T) -> T {
          let inner = self.0.get_or_init(Default::default);
          inner.replace(value)
        }
      }

      unsafe impl<T: Default> Sync for RefStaticCell<T> {}

      // This is ok to do because Wasm plugins are only ever executed on a single thread.
      // https://github.com/rust-lang/rust/issues/53639#issuecomment-790091647
      struct StaticCell<T>(std::cell::UnsafeCell<T>);

      impl<T: Default> Default for StaticCell<T> {
        fn default() -> Self {
          StaticCell(std::cell::UnsafeCell::new(T::default()))
        }
      }

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

      #[link(wasm_import_module = "dprint")]
      extern "C" {
        fn host_has_cancelled() -> i32;
      }

      fn format_with_host(
        file_path: &std::path::Path,
        file_bytes: &[u8],
        override_config: &dprint_core::configuration::ConfigKeyMap,
      ) -> anyhow::Result<Option<Vec<u8>>> {
        use std::borrow::Cow;

        #[link(wasm_import_module = "dprint")]
        extern "C" {
          fn host_read_buffer(pointer: *const u8, length: u32);
          fn host_write_buffer(pointer: *const u8);
          fn host_format(
            file_path_ptr: *const u8,
            file_path_len: u32,
            override_config_ptr: *const u8,
            override_config_len: u32,
            file_text_ptr: *const u8,
            file_text_len: u32,
          ) -> u8;
          fn host_get_formatted_text() -> u32;
          fn host_get_error_text() -> u32;
        }

        let file_path = file_path.to_string_lossy();
        let override_config = if !override_config.is_empty() {
          Cow::Owned(serde_json::to_string(override_config).unwrap())
        } else {
          Cow::Borrowed("")
        };

        return match unsafe {
          host_format(
            file_path.as_ptr(),
            file_path.len() as u32,
            override_config.as_ptr(),
            override_config.len() as u32,
            file_bytes.as_ptr(),
            file_bytes.len() as u32,
          )
        } {
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
          let (ptr, len) = set_shared_bytes(bytes);
          unsafe {
            host_read_buffer(ptr, len);
          }
        }

        fn get_string_from_host(length: u32) -> String {
          String::from_utf8(get_bytes_from_host(length)).unwrap()
        }

        fn get_bytes_from_host(len: u32) -> Vec<u8> {
          let mut index: u32 = 0;
          let ptr = clear_shared_bytes(len);
          unsafe {
            host_write_buffer(ptr);
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
      pub fn format(config_id: u32) -> (u32, *const u8, u32) {
        #[derive(Debug)]
        struct HostCancellationToken;

        impl dprint_core::plugins::CancellationToken for HostCancellationToken {
          fn is_cancelled(&self) -> bool {
            unsafe { host_has_cancelled() == 1 }
          }
        }

        let config_id = dprint_core::plugins::FormatConfigId::from_raw(config_id);
        ensure_initialized(config_id);
        let config = unsafe {
          if let Some(override_config) = OVERRIDE_CONFIG.get().take() {
            std::borrow::Cow::Owned(create_resolved_config_result(config_id, override_config).config)
          } else {
            std::borrow::Cow::Borrowed(&get_resolved_config_result(config_id).config)
          }
        };
        let file_path = unsafe { FILE_PATH.get().take().expect("Expected the file path to be set.") };
        let file_bytes = take_from_shared_bytes();

        let formatted_bytes = unsafe {
          WASM_PLUGIN
            .get()
            .format(&file_path, file_bytes, &config, &HostCancellationToken, format_with_host)
        };
        match formatted_bytes {
          Ok(None) => {
            (/* no change */ 0, get_shared_bytes_ptr(), 0)
          }
          Ok(Some(formatted_bytes)) => {
            let (ptr, len) = set_shared_bytes(formatted_bytes);
            (/* change */ 1, ptr, len)
          }
          Err(err_text) => {
            let bytes = err_text.to_string().into_bytes();
            let (ptr, len) = set_shared_bytes(bytes);
            (/* error */ 2, ptr, len)
          }
        }
      }

      // INFORMATION & CONFIGURATION

      static RESOLVE_CONFIGURATION_RESULT: RefStaticCell<
        std::collections::HashMap<dprint_core::plugins::FormatConfigId, dprint_core::plugins::PluginResolveConfigurationResult<Configuration>>,
      > = RefStaticCell::new();

      #[no_mangle]
      pub fn get_plugin_info() -> (*const u8, u32) {
        use dprint_core::plugins::PluginInfo;
        let plugin_info = unsafe { WASM_PLUGIN.get().plugin_info() };
        let info_json = serde_json::to_string(&plugin_info).unwrap();
        set_shared_bytes_str(info_json)
      }

      #[no_mangle]
      pub fn get_license_text() -> (*const u8, u32) {
        set_shared_bytes_str(unsafe { WASM_PLUGIN.get().license_text() })
      }

      #[no_mangle]
      pub fn get_resolved_config(config_id: u32) -> (*const u8, u32) {
        let config_id = dprint_core::plugins::FormatConfigId::from_raw(config_id);
        let bytes = serde_json::to_vec(&get_resolved_config_result(config_id).config).unwrap();
        set_shared_bytes(bytes)
      }

      #[no_mangle]
      pub fn get_config_diagnostics(config_id: u32) -> (*const u8, u32) {
        let config_id = dprint_core::plugins::FormatConfigId::from_raw(config_id);
        let bytes = serde_json::to_vec(&get_resolved_config_result(config_id).diagnostics).unwrap();
        set_shared_bytes(bytes)
      }

      #[no_mangle]
      pub fn get_config_file_matching(config_id: u32) -> (*const u8, u32) {
        let config_id = dprint_core::plugins::FormatConfigId::from_raw(config_id);
        let bytes = serde_json::to_vec(&get_resolved_config_result(config_id).file_matching).unwrap();
        set_shared_bytes(bytes)
      }

      fn get_resolved_config_result<'a>(
        config_id: dprint_core::plugins::FormatConfigId,
      ) -> &'a dprint_core::plugins::PluginResolveConfigurationResult<Configuration> {
        unsafe {
          ensure_initialized(config_id);
          return RESOLVE_CONFIGURATION_RESULT.get().get(&config_id).unwrap();
        }
      }

      fn ensure_initialized(config_id: dprint_core::plugins::FormatConfigId) {
        unsafe {
          if !RESOLVE_CONFIGURATION_RESULT.get().contains_key(&config_id) {
            let config_result = create_resolved_config_result(config_id, dprint_core::configuration::ConfigKeyMap::new());
            RESOLVE_CONFIGURATION_RESULT.get().insert(config_id, config_result);
          }
        }
      }

      fn create_resolved_config_result(
        config_id: dprint_core::plugins::FormatConfigId,
        override_config: dprint_core::configuration::ConfigKeyMap,
      ) -> dprint_core::plugins::PluginResolveConfigurationResult<Configuration> {
        unsafe {
          if let Some(config) = UNRESOLVED_CONFIG.get().get(&config_id) {
            let mut plugin_config = config.plugin.clone();
            for (key, value) in override_config {
              plugin_config.insert(key, value);
            }
            return WASM_PLUGIN.get().resolve_config(plugin_config, &config.global);
          }
        }

        panic!("Plugin must have config set before use (id: {:?}).", config_id);
      }

      // INITIALIZATION

      static UNRESOLVED_CONFIG: RefStaticCell<std::collections::HashMap<dprint_core::plugins::FormatConfigId, dprint_core::plugins::RawFormatConfig>> =
        RefStaticCell::new();

      #[no_mangle]
      pub fn register_config(config_id: u32) {
        let config_id = dprint_core::plugins::FormatConfigId::from_raw(config_id);
        let bytes = take_from_shared_bytes();
        let config: dprint_core::plugins::RawFormatConfig = serde_json::from_slice(&bytes).unwrap();
        unsafe {
          UNRESOLVED_CONFIG.get().insert(config_id, config);
          RESOLVE_CONFIGURATION_RESULT.get().remove(&config_id); // clear
        }
      }

      #[no_mangle]
      pub fn release_config(config_id: u32) {
        let config_id = dprint_core::plugins::FormatConfigId::from_raw(config_id);
        unsafe {
          UNRESOLVED_CONFIG.get().remove(&config_id);
          RESOLVE_CONFIGURATION_RESULT.get().remove(&config_id);
        }
      }

      // LOW LEVEL SENDING AND RECEIVING

      static SHARED_BYTES: StaticCell<Vec<u8>> = StaticCell::new(Vec::new());

      #[no_mangle]
      pub fn dprint_plugin_version_4() -> u32 {
        dprint_core::plugins::wasm::PLUGIN_SYSTEM_SCHEMA_VERSION
      }

      #[no_mangle]
      pub fn clear_shared_bytes(size: u32) -> *const u8 {
        SHARED_BYTES.replace(vec![0; size as usize]);
        get_shared_bytes_ptr()
      }

      fn get_shared_bytes_ptr() -> *const u8 {
        unsafe { SHARED_BYTES.get().as_ptr() }
      }

      fn take_string_from_shared_bytes() -> String {
        String::from_utf8(take_from_shared_bytes()).unwrap()
      }

      fn take_from_shared_bytes() -> Vec<u8> {
        SHARED_BYTES.replace(Vec::new())
      }

      fn set_shared_bytes_str(text: String) -> (*const u8, u32) {
        set_shared_bytes(text.into_bytes())
      }

      fn set_shared_bytes(bytes: Vec<u8>) -> (*const u8, u32) {
        let length = bytes.len() as u32;
        SHARED_BYTES.replace(bytes);
        (get_shared_bytes_ptr(), length)
      }
    };
  }
}
