extern crate dprint_plugin_typescript;

use dprint_plugin_typescript::*;

#[test]
fn it_testing() {
    let result = format_text("test.ts".into(), " \n '5' ;  ".into()).unwrap();
    assert_eq!(result, "'5';");
}
