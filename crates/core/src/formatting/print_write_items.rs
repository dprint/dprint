use super::WriteItem;

/// Prints writer items to a string.
pub fn print_write_items<'a>(write_items: impl Iterator<Item = &'a WriteItem<'a>>, options: WriteItemsPrinterOptions) -> String {
  WriteItemsPrinter::new(options).write_items_to_string(write_items)
}

pub struct WriteItemsPrinterOptions {
  /// The number of spaces to use when indenting when use_tabs is false,
  /// otherwise the number of columns to count a tab for when use_tabs is true.
  pub indent_width: u8,
  /// Whether to use tabs for indenting.
  pub use_tabs: bool,
  /// The newline character to use when doing a new line.
  pub new_line_text: &'static str,
}

pub struct WriteItemsPrinter {
  indent_string: String,
  new_line_text: &'static str,
}

impl WriteItemsPrinter {
  pub fn new(options: WriteItemsPrinterOptions) -> Self {
    WriteItemsPrinter {
      indent_string: if options.use_tabs {
        String::from("\t")
      } else {
        " ".repeat(options.indent_width as usize)
      },
      new_line_text: options.new_line_text,
    }
  }

  pub fn write_items_to_string<'a>(&self, write_items: impl Iterator<Item = &'a WriteItem<'a>>) -> String {
    // todo: faster string manipulation? or is this as good as it gets?
    let mut final_string = String::new();

    for item in write_items.into_iter() {
      self.write_to_string(&mut final_string, item);
    }

    final_string
  }

  #[inline]
  pub fn write_to_string(&self, final_string: &mut String, item: &WriteItem) {
    // todo: cache indent strings?
    match item {
      WriteItem::Indent(times) => final_string.push_str(&self.indent_string.repeat(*times as usize)),
      WriteItem::NewLine => final_string.push_str(&self.new_line_text),
      WriteItem::Tab => final_string.push('\t'),
      WriteItem::Space => final_string.push(' '),
      WriteItem::String(text) => final_string.push_str(&text.text),
    }
  }
}
