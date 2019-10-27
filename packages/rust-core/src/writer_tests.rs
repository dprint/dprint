use super::writer::*;

// todo: some basic unit tests just to make sure I'm not way off

#[test]
fn write_singleword_writes() {
    let mut writer = create_writer();
    writer.write("test");
    assert_eq!(writer.to_string(), String::from("test"));
}

#[test]
fn write_multiple_lines_writes() {
    let mut writer = create_writer();
    writer.write("1");
    writer.new_line();
    writer.write("2");
    assert_eq!(writer.to_string(), String::from("1\n2"));
}

#[test]
fn write_indented_writes() {
    let mut writer = create_writer();
    writer.write("1");
    writer.new_line();
    writer.start_indent();
    writer.write("2");
    writer.finish_indent();
    writer.new_line();
    writer.write("3");
    assert_eq!(writer.to_string(), String::from("1\n  2\n3"));
}

#[test]
fn write_singleindent_writes() {
    let mut writer = create_writer();
    writer.single_indent();
    writer.write("t");
    assert_eq!(writer.to_string(), String::from("  t"));
}

#[test]
fn markexpectnewline_writesnewline() {
    let mut writer = create_writer();
    writer.write("1");
    writer.mark_expect_new_line();
    assert_eq!(writer.to_string(), String::from("1"));
    writer.write("2");
    assert_eq!(writer.to_string(), String::from("1\n2"));
}

fn create_writer() -> Writer {
    Writer::new(WriterOptions {
        indent_width: 2,
        use_tabs: false,
        newline_kind: "\n",
    })
}
