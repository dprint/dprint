use super::writer::*;
use super::print_write_items;
use super::PrintWriteItemsOptions;

// todo: some basic unit tests just to make sure I'm not way off

#[test]
fn write_singleword_writes() {
    let mut writer = create_writer();
    writer.write(&String::from("test"));
    assert_writer_equal(&writer, "test");
}

#[test]
fn write_multiple_lines_writes() {
    let mut writer = create_writer();
    writer.write(&String::from("1"));
    writer.new_line();
    writer.write(&String::from("2"));
    assert_writer_equal(&writer, "1\n2");
}

#[test]
fn write_indented_writes() {
    let mut writer = create_writer();
    writer.write(&String::from("1"));
    writer.new_line();
    writer.start_indent();
    writer.write(&String::from("2"));
    writer.finish_indent();
    writer.new_line();
    writer.write(&String::from("3"));
    assert_writer_equal(&writer, "1\n  2\n3");
}

#[test]
fn write_singleindent_writes() {
    let mut writer = create_writer();
    writer.single_indent();
    writer.write(&String::from("t"));
    assert_writer_equal(&writer, "  t");
}

#[test]
fn markexpectnewline_writesnewline() {
    let mut writer = create_writer();
    writer.write(&String::from("1"));
    writer.mark_expect_new_line();
    assert_writer_equal(&writer, "1");
    writer.write(&String::from("2"));
    assert_writer_equal(&writer, "1\n2");
}

fn assert_writer_equal(writer: &Writer<String>, text: &str) {
    let result = print_write_items(writer.get_items_copy(), PrintWriteItemsOptions {
        indent_width: 2,
        use_tabs: false,
        newline_kind: "\n",
    });
    assert_eq!(result, String::from(text));
}

fn create_writer() -> Writer<String> {
    Writer::new(WriterOptions { indent_width: 2 })
}
