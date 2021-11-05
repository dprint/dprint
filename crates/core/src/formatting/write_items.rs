use super::{PrintOptions, StringContainer};

#[derive(Clone, Copy)]
pub enum WriteItem<'a> {
  String(&'a StringContainer),
  Indent(u8),
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
    items.fold(String::new(), |acc, item| match item {
      WriteItem::Indent(n) => match self.indent {
        Indentation::Tabs => acc + &str::repeat("\t", n as usize),
        Indentation::Spaces(width) => acc + &str::repeat(" ", width * n as usize),
      },
      WriteItem::NewLine => acc + self.newline,
      WriteItem::Tab => acc + "\t",
      WriteItem::Space => acc + " ",
      WriteItem::String(StringContainer { text, .. }) => acc + text,
    })
  }
}

impl From<&PrintOptions> for WriteItemsPrinter {
  fn from(value: &PrintOptions) -> Self {
    Self::new(value.indent_width, value.use_tabs, value.new_line_text)
  }
}
