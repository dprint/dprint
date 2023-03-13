use std::path::PathBuf;

fn main() {
  println!("cargo:rustc-env=TARGET={}", std::env::var("TARGET").unwrap());
}
