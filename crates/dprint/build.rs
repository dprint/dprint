#[allow(clippy::disallowed_methods)]
fn main() {
  println!("cargo:rustc-env=TARGET={}", std::env::var("TARGET").unwrap());
  println!("cargo:rustc-env=RUSTC_VERSION_TEXT={}", get_rustc_version());
}

fn get_rustc_version() -> String {
  let output = std::process::Command::new("rustc").arg("-V").output().unwrap();
  String::from_utf8(output.stdout).unwrap().trim().to_string()
}
