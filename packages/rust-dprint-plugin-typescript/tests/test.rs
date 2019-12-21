extern crate dprint_plugin_typescript;
extern crate dprint_development;

use dprint_plugin_typescript::*;
use dprint_development::*;
use std::io;
use std::fs::{self, DirEntry};
use std::path::Path;

#[test]
fn it_testing() {
    let result = format_text("test.ts".into(), "/* test */ // 2\nfunction test() { //3\n}\n".into()).unwrap();
    //assert_eq!(result, "'use strict';");
}

#[test]
fn test_specs() {
    let spec_files = get_spec_files();
    assert_eq!(2, spec_files.len());
}

fn get_spec_files() -> Vec<(String, String)> {
    return read_dir_recursively(&Path::new("./tests/specs"));

    fn read_dir_recursively(dir_path: &Path) -> Vec<(String, String)> {
        let mut result = Vec::new();

        for entry in dir_path.read_dir().expect("read dir failed") {
            if let Ok(entry) = entry {
                let entry_path = entry.path();
                if entry_path.is_file() {
                    result.push((entry_path.to_str().unwrap().into(), fs::read_to_string(entry_path).unwrap().into()));
                } else {
                    result.extend(read_dir_recursively(&entry_path));
                }
            }
        }

        result
    }
}
