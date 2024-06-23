/// The plugin system schema version that is incremented
/// when there are any breaking changes.
pub const PLUGIN_SYSTEM_SCHEMA_VERSION: u32 = 4;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
extern "C" {
  fn fd_write(fd: i32, iovs: *const crate::plugins::wasm::Iovec, iovs_len: i32, nwritten: *mut i32) -> i32;
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "data")]
pub enum JsonResponse {
  #[serde(rename = "ok")]
  Ok(serde_json::Value),
  #[serde(rename = "err")]
  Err(String),
}

pub struct WasiPrintFd(pub i32);

impl std::io::Write for WasiPrintFd {
  fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
      let iovec = Iovec {
        buf: buf.as_ptr(),
        buf_len: buf.len() as u32,
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
  pub buf_len: u32,
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

      fn format_with_host(request: dprint_core::plugins::SyncHostFormatRequest) -> anyhow::Result<Option<Vec<u8>>> {
        use std::borrow::Cow;

        #[link(wasm_import_module = "dprint")]
        extern "C" {
          fn host_read_buffer(pointer: *const u8, length: u32);
          fn host_write_buffer(pointer: *const u8);
          fn host_format(
            file_path_ptr: *const u8,
            file_path_len: u32,
            start_range: u32,
            end_range: u32,
            override_config_ptr: *const u8,
            override_config_len: u32,
            file_text_ptr: *const u8,
            file_text_len: u32,
          ) -> u8;
          fn host_get_formatted_text() -> u32;
          fn host_get_error_text() -> u32;
        }

        let file_path = request.file_path.to_string_lossy();
        let override_config = if !request.override_config.is_empty() {
          Cow::Owned(serde_json::to_string(request.override_config).unwrap())
        } else {
          Cow::Borrowed("")
        };
        let range = request.range.unwrap_or(0..request.file_bytes.len());

        return match unsafe {
          host_format(
            file_path.as_ptr(),
            file_path.len() as u32,
            range.start as u32,
            range.end as u32,
            override_config.as_ptr(),
            override_config.len() as u32,
            request.file_bytes.as_ptr(),
            request.file_bytes.len() as u32,
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
          let length = set_shared_bytes(bytes);
          unsafe {
            host_read_buffer(get_shared_bytes_ptr(), length as u32);
          }
        }

        fn get_string_from_host(length: u32) -> String {
          String::from_utf8(get_bytes_from_host(length)).unwrap()
        }

        fn get_bytes_from_host(length: u32) -> Vec<u8> {
          let mut index: u32 = 0;
          let ptr = clear_shared_bytes(length as usize);
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
      pub fn format(config_id: u32) -> u8 {
        format_inner(config_id, None)
      }

      #[no_mangle]
      pub fn format_range(config_id: u32, range_start: u32, range_end: u32) -> u8 {
        format_inner(config_id, Some(range_start as usize..range_end as usize))
      }

      fn format_inner(config_id: u32, range: dprint_core::plugins::FormatRange) -> u8 {
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

        let request = dprint_core::plugins::SyncFormatRequest::<Configuration> {
          file_path: &file_path,
          file_bytes,
          config: &config,
          config_id,
          range,
          token: &HostCancellationToken,
        };
        let formatted_text = unsafe { WASM_PLUGIN.get().format(request, format_with_host) };
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

      static RESOLVE_CONFIGURATION_RESULT: RefStaticCell<
        std::collections::HashMap<dprint_core::plugins::FormatConfigId, dprint_core::plugins::PluginResolveConfigurationResult<Configuration>>,
      > = RefStaticCell::new();

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
      pub fn get_resolved_config(config_id: u32) -> usize {
        let config_id = dprint_core::plugins::FormatConfigId::from_raw(config_id);
        let bytes = serde_json::to_vec(&get_resolved_config_result(config_id).config).unwrap();
        set_shared_bytes(bytes)
      }

      #[no_mangle]
      pub fn get_config_diagnostics(config_id: u32) -> usize {
        let config_id = dprint_core::plugins::FormatConfigId::from_raw(config_id);
        let bytes = serde_json::to_vec(&get_resolved_config_result(config_id).diagnostics).unwrap();
        set_shared_bytes(bytes)
      }

      #[no_mangle]
      pub fn get_config_file_matching(config_id: u32) -> usize {
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

      #[no_mangle]
      pub fn check_config_updates() -> usize {
        fn try_check_config_updates(bytes: &[u8]) -> anyhow::Result<serde_json::Value> {
          let message: dprint_core::plugins::CheckConfigUpdatesMessage = serde_json::from_slice(&bytes)?;
          let result = unsafe { WASM_PLUGIN.get().check_config_updates(message) }?;
          Ok(serde_json::to_value(&result)?)
        }

        let bytes = take_from_shared_bytes();
        let bytes = serde_json::to_vec(&match try_check_config_updates(&bytes) {
          Ok(value) => dprint_core::plugins::wasm::JsonResponse::Ok(value),
          Err(err) => dprint_core::plugins::wasm::JsonResponse::Err(err.to_string()),
        })
        .unwrap();
        set_shared_bytes(bytes)
      }

      // LOW LEVEL SENDING AND RECEIVING

      static SHARED_BYTES: StaticCell<Vec<u8>> = StaticCell::new(Vec::new());

      #[no_mangle]
      pub fn dprint_plugin_version_4() -> u32 {
        dprint_core::plugins::wasm::PLUGIN_SYSTEM_SCHEMA_VERSION
      }

      #[no_mangle]
      pub fn get_shared_bytes_ptr() -> *const u8 {
        unsafe { SHARED_BYTES.get().as_ptr() }
      }

      #[no_mangle]
      pub fn clear_shared_bytes(size: usize) -> *const u8 {
        SHARED_BYTES.replace(vec![0; size]);
        unsafe { SHARED_BYTES.get().as_ptr() }
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
