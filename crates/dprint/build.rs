#[allow(clippy::disallowed_methods)]
fn main() {
  println!("cargo:rustc-env=TARGET={}", std::env::var("TARGET").unwrap());
  println!("cargo:rustc-env=RUSTC_VERSION_TEXT={}", get_rustc_version());
  set_wasm_backend_cfg();
  set_windows_delay_load_dlls();
}

// Select the wasm backend. wasmtime's Cranelift backend has native codegen for
// x86_64, aarch64, riscv64 and s390x. On powerpc64/loongarch64 it has no native
// backend, and in the Android sandbox the signal-based trap handling that native
// code relies on is unavailable, so those targets compile to wasmtime's portable
// Pulley bytecode and interpret it instead (see the `use_pulley` cfg in
// crates/dprint/src/plugins/implementations/wasm/load_instance.rs).
#[allow(clippy::disallowed_methods)]
fn set_wasm_backend_cfg() {
  println!("cargo:rustc-check-cfg=cfg(use_pulley)");
  let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
  let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
  let use_pulley = matches!(arch.as_str(), "powerpc64" | "loongarch64") || os == "android";
  if use_pulley {
    println!("cargo:rustc-cfg=use_pulley");
  }
}

// delay load DLLs that aren't needed on the common startup path so they
// don't slow down Windows startup. these are only resolved on first use.
// the `windows_dll_imports` test verifies the eager/delay split.
#[allow(clippy::disallowed_methods)]
fn set_windows_delay_load_dlls() {
  let is_windows_msvc = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
  if !is_windows_msvc {
    return;
  }

  let dlls = [
    "ws2_32",   // networking—only needed when downloading plugins
    "crypt32",  // TLS certificate store—only needed when downloading plugins
    "combase",  // COM/OLE—off the common startup path
    "oleaut32", // COM/OLE—off the common startup path
    "pdh",      // sysinfo CPU usage—only while throttling CPU during a long format run
    "powrprof", // sysinfo CPU usage—only while throttling CPU during a long format run
    "psapi",    // sysinfo process info—only during plugin cache cleanup of a locked dir
  ];
  for dll in dlls {
    println!("cargo:rustc-link-arg-bin=dprint=/delayload:{dll}.dll");
  }
  // link the delay load helper that the above flags require
  println!("cargo:rustc-link-arg-bin=dprint=delayimp.lib");
}

fn get_rustc_version() -> String {
  let output = std::process::Command::new("rustc").arg("-V").output().unwrap();
  String::from_utf8(output.stdout).unwrap().trim().to_string()
}
