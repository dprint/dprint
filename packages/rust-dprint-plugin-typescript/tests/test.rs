extern crate dprint_plugin_typescript;

use dprint_plugin_typescript::*;

#[test]
fn it_testing() {
    let result = format_text("test.ts".into(), "/* test */ // 2\nfunction test() { //3\n}\n".into()).unwrap();
    assert_eq!(result, "'use strict';");
}
