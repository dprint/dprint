use std::rc::Rc;
use super::writer::*;
use super::print_write_items;
use super::StringContainer;
use super::PrintWriteItemsOptions;

// todo: some basic unit tests just to make sure I'm not way off

#[test]
fn write_singleword_writes() {
    let mut writer = create_writer();
    write_text(&mut writer, "test");
    assert_writer_equal(writer, "test");
}

#[test]
fn write_multiple_lines_writes() {
    let mut writer = create_writer();
    write_text(&mut writer, "1");
    writer.new_line();
    write_text(&mut writer, "2");
    assert_writer_equal(writer, "1\n2");
}

#[test]
fn write_indented_writes() {
    let mut writer = create_writer();
    write_text(&mut writer, "1");
    writer.new_line();
    writer.start_indent();
    write_text(&mut writer, "2");
    writer.finish_indent();
    writer.new_line();
    write_text(&mut writer, "3");
    assert_writer_equal(writer, "1\n  2\n3");
}

#[test]
fn write_singleindent_writes() {
    let mut writer = create_writer();
    writer.single_indent();
    write_text(&mut writer, "t");
    assert_writer_equal(writer, "  t");
}

#[test]
fn markexpectnewline_writesnewline() {
    let mut writer = create_writer();
    write_text(&mut writer, "1");
    writer.mark_expect_new_line();
    write_text(&mut writer, "2");
    assert_writer_equal(writer, "1\n2");
}

fn assert_writer_equal(writer: Writer<String>, text: &str) {
    let result = print_write_items(writer.get_items(), PrintWriteItemsOptions {
        indent_width: 2,
        use_tabs: false,
        new_line_text: "\n",
    });
    assert_eq!(result, String::from(text));
}

fn write_text(writer: &mut Writer<String>, text: &'static str) {
    writer.write(Rc::new(StringContainer::new(String::from(text))));
}

fn create_writer() -> Writer<String> {
    Writer::new(WriterOptions { indent_width: 2 })
}
