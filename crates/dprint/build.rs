#[allow(clippy::disallowed_methods)]
fn main() {
  println!("cargo:rustc-env=TARGET={}", std::env::var("TARGET").unwrap());
  println!("cargo:rustc-env=RUSTC_VERSION_TEXT={}", get_rustc_version());
  set_windows_delay_load_dlls();
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
    "pdh",      // sysinfo (CPU/process usage)—only needed while formatting
    "powrprof", // sysinfo (CPU/process usage)—only needed while formatting
    "psapi",    // sysinfo (process refresh)—only needed while formatting
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
