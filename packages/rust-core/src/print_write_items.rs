use super::StringRef;
use super::WriteItem;
use std::rc::Rc;

pub struct PrintWriteItemsOptions {
    /// The number of spaces to use when indenting when use_tabs is false,
    /// otherwise the number of columns to count a tab for when use_tabs is true.
    pub indent_width: u8,
    /// Whether to use tabs for indenting.
    pub use_tabs: bool,
    /// The newline character to use when doing a new line.
    pub newline_kind: &'static str,
}

/// Prints string based writer items.
pub fn print_write_items<T>(write_items: impl Iterator<Item = WriteItem<T>>, options: PrintWriteItemsOptions) -> String where T : StringRef {
    // todo: faster string manipulation?
    let mut final_string = String::new();
    let indent_string = if options.use_tabs { String::from("\t") } else { " ".repeat(options.indent_width as usize) };

    for item in write_items.into_iter() {
        match item {
            WriteItem::Indent => final_string.push_str(&indent_string),
            WriteItem::NewLine => final_string.push_str(&options.newline_kind),
            WriteItem::Tab => final_string.push_str("\t"),
            WriteItem::Space => final_string.push_str(" "),
            WriteItem::String(text) => {
                // todo: how to move it into final_string? (performance?)
                final_string.push_str(&match Rc::try_unwrap(text) {
                    Result::Ok(moved_text) => moved_text.get_text(),
                    // this means the string is being referenced in multiple places, which is expected so make a clone
                    Result::Err(text) => text.get_text_clone(),
                });
            },
        }
    }

    final_string
}
