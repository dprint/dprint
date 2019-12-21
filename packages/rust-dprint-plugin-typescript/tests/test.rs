extern crate dprint_plugin_typescript;
extern crate dprint_development;

use dprint_plugin_typescript::*;
use dprint_development::*;
use std::fs::{self};
use std::path::Path;

#[test]
fn it_testing() {
    //let result = format_text("test.ts".into(), "/* test */ // 2\nfunction test() { //3\n}\n".into()).unwrap();
    //assert_eq!(result, "'use strict';");
}

#[test]
fn test_specs() {
    let specs = get_specs();
    for (file_path, spec) in specs.iter().filter(|(_, spec)| !spec.skip) {
        let config = resolve_config(&spec.config);
        let result = format_text(&spec.file_name, &spec.file_text, &config).expect(format!("Could not parse spec '{}' in {}", spec.message, file_path).as_str());
        if result != spec.expected_text {
            panic!("spec assertion failed ({}):
 expected: `{:?}`,
   actual: `{:?}`", spec.message, spec.expected_text, result);
        }
    }
}

fn get_specs() -> Vec<(String, Spec)> {
    let mut result: Vec<(String, Spec)> = Vec::new();
    let spec_files = get_spec_files();
    for (file_path, text) in spec_files {
        let specs = parse_specs(text, ParseSpecOptions { default_file_name: "file.ts" });
        for spec in specs {
            result.push((file_path.clone(), spec));
        }
    }

    if result.iter().any(|(_, spec)| spec.is_only) {
        result.into_iter().filter(|(_, spec)| spec.is_only).collect()
    } else {
        result
    }
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
