use dprint_cli_core::logging::render_text_items_with_width;
use dprint_cli_core::logging::LoggerTextItem;

pub struct TableText {
  pub lines: Vec<String>,
  pub hanging_indent: u16,
}

impl TableText {
  pub fn render(&self, indent: u16, terminal_width: Option<u16>) -> String {
    let text_items = self.get_logger_text_items(indent);
    render_text_items_with_width(&text_items, terminal_width)
  }

  pub fn get_logger_text_items(&self, indent: u16) -> Vec<LoggerTextItem> {
    let mut text_items = Vec::new();
    for line in self.lines.iter() {
      let mut text = String::new();
      if indent > 0 {
        text.push_str(&" ".repeat(indent as usize));
      }
      text.push_str(line);
      text_items.push(LoggerTextItem::HangingText {
        text,
        indent: indent + self.hanging_indent,
      });
    }
    text_items
  }
}

pub fn get_table_text(items: Vec<(&str, &str)>) -> TableText {
  let largest_name_len = get_largest_string_len(items.iter().map(|(key, _)| *key));

  let lines = items
    .iter()
    .map(|(key, value)| {
      let mut text = String::new();
      text.push_str(key);
      text.push_str(&" ".repeat(largest_name_len - key.len() + 1));
      text.push_str(value);
      text
    })
    .collect();

  TableText {
    lines,
    hanging_indent: (largest_name_len + 1) as u16,
  }
}

fn get_largest_string_len<'a>(items: impl Iterator<Item = &'a str>) -> usize {
  let mut key_lens = items.map(|item| item.chars().count()).collect::<Vec<_>>();
  key_lens.sort_unstable();
  key_lens.pop().unwrap_or(0)
}
