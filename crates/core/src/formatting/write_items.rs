use super::PrintOptions;
use super::StringContainer;

#[derive(Clone, Copy)]
pub enum WriteItem<'a> {
  String(&'a StringContainer<'a>),
  Indent(u8),
  /// Columns of spaces after the indentation.
  Align(u32),
  NewLine,
  Tab,
  Space,
}

pub enum Indentation {
  /// Indent with tabs.
  Tabs,
  /// Indent with spaces. Specifies the number of spaces that make up each indentation level.
  Spaces(usize),
}

pub struct WriteItemsPrinter {
  /// Indentation to apply.
  pub indent: Indentation,
  /// Character to use for a newline.
  pub newline: &'static str,
}

impl WriteItemsPrinter {
  pub fn new(indent_width: u8, use_tabs: bool, newline: &'static str) -> Self {
    Self {
      indent: match use_tabs {
        true => Indentation::Tabs,
        false => Indentation::Spaces(indent_width as usize),
      },
      newline,
    }
  }

  pub fn print<'a>(&self, items: impl Iterator<Item = WriteItem<'a>>) -> String {
    let mut text = String::new();
    for item in items {
      match item {
        WriteItem::Indent(n) => match self.indent {
          Indentation::Tabs => text.extend(std::iter::repeat_n('\t', n as usize)),
          Indentation::Spaces(width) => text.extend(std::iter::repeat_n(' ', width * n as usize)),
        },
        WriteItem::Align(columns) => text.extend(std::iter::repeat_n(' ', columns as usize)),
        WriteItem::NewLine => text.push_str(self.newline),
        WriteItem::Tab => text.push('\t'),
        WriteItem::Space => text.push(' '),
        WriteItem::String(StringContainer { text: item_text, .. }) => text.push_str(item_text),
      }
    }
    text
  }
}

impl From<&PrintOptions> for WriteItemsPrinter {
  fn from(value: &PrintOptions) -> Self {
    Self::new(value.indent_width, value.use_tabs, value.new_line_text)
  }
}
