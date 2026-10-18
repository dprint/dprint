/// The plugin system schema version that is incremented
/// when there are any breaking changes.
pub const PLUGIN_SYSTEM_SCHEMA_VERSION: u32 = 4;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
mod state;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub use state::WasmPluginState;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
unsafe extern "C" {
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
      $crate::generate_plugin_code!($wasm_plugin_struct, $wasm_plugin_creation, Configuration);
    };
    ($wasm_plugin_struct:ident, $wasm_plugin_creation:expr, $wasm_plugin_config:ident) => {
      // Keep this expansion minimal because it's linted with the plugin's lint
      // configuration (ex. clippy's `disallowed_types`). The implementation lives
      // in dprint-core instead. Note that adding an `#[allow(...)]` here would
      // conflict with a plugin forbidding any of the allowed lints.
      const _: () = {
        static STATE: $crate::plugins::wasm::WasmPluginState<$wasm_plugin_struct, $wasm_plugin_config> =
          $crate::plugins::wasm::WasmPluginState::new(|| $wasm_plugin_creation);

        // FORMATTING

        #[unsafe(no_mangle)]
        pub fn set_override_config() {
          STATE.set_override_config()
        }

        #[unsafe(no_mangle)]
        pub fn set_file_path() {
          STATE.set_file_path()
        }

        #[unsafe(no_mangle)]
        pub fn format(config_id: u32) -> u8 {
          STATE.format(config_id, None)
        }

        #[unsafe(no_mangle)]
        pub fn format_range(config_id: u32, range_start: u32, range_end: u32) -> u8 {
          STATE.format(config_id, Some(range_start as usize..range_end as usize))
        }

        #[unsafe(no_mangle)]
        pub fn get_formatted_text() -> usize {
          STATE.get_formatted_text()
        }

        #[unsafe(no_mangle)]
        pub fn get_error_text() -> usize {
          STATE.get_error_text()
        }

        // INFORMATION & CONFIGURATION

        #[unsafe(no_mangle)]
        pub fn get_plugin_info() -> usize {
          STATE.get_plugin_info()
        }

        #[unsafe(no_mangle)]
        pub fn get_license_text() -> usize {
          STATE.get_license_text()
        }

        #[unsafe(no_mangle)]
        pub fn get_resolved_config(config_id: u32) -> usize {
          STATE.get_resolved_config(config_id)
        }

        #[unsafe(no_mangle)]
        pub fn get_config_diagnostics(config_id: u32) -> usize {
          STATE.get_config_diagnostics(config_id)
        }

        #[unsafe(no_mangle)]
        pub fn get_config_file_matching(config_id: u32) -> usize {
          STATE.get_config_file_matching(config_id)
        }

        // INITIALIZATION

        #[unsafe(no_mangle)]
        pub fn register_config(config_id: u32) {
          STATE.register_config(config_id)
        }

        #[unsafe(no_mangle)]
        pub fn release_config(config_id: u32) {
          STATE.release_config(config_id)
        }

        #[unsafe(no_mangle)]
        pub fn check_config_updates() -> usize {
          STATE.check_config_updates()
        }

        // LOW LEVEL SENDING AND RECEIVING

        #[unsafe(no_mangle)]
        pub fn dprint_plugin_version_4() -> u32 {
          $crate::plugins::wasm::PLUGIN_SYSTEM_SCHEMA_VERSION
        }

        #[unsafe(no_mangle)]
        pub fn get_shared_bytes_ptr() -> *const u8 {
          STATE.get_shared_bytes_ptr()
        }

        #[unsafe(no_mangle)]
        pub fn clear_shared_bytes(size: usize) -> *const u8 {
          STATE.clear_shared_bytes(size)
        }
      };
    };
  }
}
