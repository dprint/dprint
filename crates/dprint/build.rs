use std::path::PathBuf;

fn main() {
  println!("cargo:rustc-env=TARGET={}", std::env::var("TARGET").unwrap());

  let dprint_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());

  // find the text:
  //
  // name = "wasmer-compiler"
  // version = "2.3.0"
  let lock_file_text = std::fs::read_to_string(dprint_dir.join("../../Cargo.lock")).unwrap();
  let search_text = r#"name = "wasmer-compiler""#;
  let text = &lock_file_text[lock_file_text.find(search_text).unwrap()..];
  let next_line = text.lines().nth(1).unwrap();
  let version_text = "version = \"";
  assert!(next_line.starts_with(version_text));
  assert!(next_line.ends_with('"'));
  let wasmer_compiler_version = &next_line[version_text.len()..next_line.len() - 1];
  // this is to just be notified when the wasmer-compiler version
  // changes, so just bump this if it fails
  debug_assert_eq!(wasmer_compiler_version, "2.3.0");
  println!("cargo:rustc-env=WASMER_COMPILER_VERSION={}", version_text);
}
