use anyhow::Result;
use anyhow::bail;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyModifiers;

use crate::utils::terminal::get_terminal_size;
use crate::utils::terminal::read_terminal_key_press;

use super::Logger;
use super::LoggerRefreshItemKind;
use super::LoggerTextItem;

struct MultiSelectData<'a> {
  prompt: &'a str,
  item_hanging_indent: u16,
  items: Vec<(bool, &'a String)>,
  /// Text typed by the user to narrow down the visible items.
  filter: String,
  /// Index into the currently visible (filtered) items.
  active_index: usize,
  /// First visible (filtered) item, used to scroll long lists.
  scroll_offset: usize,
}

pub fn show_multi_select(logger: &Logger, context_name: &str, prompt: &str, item_hanging_indent: u16, items: Vec<(bool, &String)>) -> Result<Vec<usize>> {
  let mut data = MultiSelectData {
    prompt,
    items,
    item_hanging_indent,
    filter: String::new(),
    active_index: 0,
    scroll_offset: 0,
  };

  loop {
    let visible = visible_indexes(&data);
    // keep the active index and scrolling within the visible items
    if data.active_index >= visible.len() {
      data.active_index = visible.len().saturating_sub(1);
    }
    let max_visible_rows = max_visible_rows(&data);
    update_scroll_offset(&mut data, visible.len(), max_visible_rows);

    let text_items = render_multi_select(&data, &visible, max_visible_rows);
    logger.set_refresh_item(LoggerRefreshItemKind::Selection, text_items);

    if let Event::Key(key_event) = read_terminal_key_press()? {
      // ctrl+c cancels
      if key_event.modifiers.contains(KeyModifiers::CONTROL) && matches!(key_event.code, KeyCode::Char('c')) {
        logger.remove_refresh_item(LoggerRefreshItemKind::Selection);
        bail!("Selection cancelled.");
      }
      match &key_event.code {
        KeyCode::Up => {
          if !visible.is_empty() {
            data.active_index = (data.active_index + visible.len() - 1) % visible.len();
          }
        }
        KeyCode::Down => {
          if !visible.is_empty() {
            data.active_index = (data.active_index + 1) % visible.len();
          }
        }
        KeyCode::Char(' ') => {
          // toggle the active item's selection
          if let Some(&item_index) = visible.get(data.active_index) {
            data.items[item_index].0 = !data.items[item_index].0;
          }
        }
        KeyCode::Backspace => {
          if data.filter.pop().is_some() {
            data.active_index = 0;
            data.scroll_offset = 0;
          }
        }
        KeyCode::Char(c) => {
          // any other printable character narrows down the list
          if !key_event.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) {
            data.filter.push(*c);
            data.active_index = 0;
            data.scroll_offset = 0;
          }
        }
        KeyCode::Enter => {
          break;
        }
        KeyCode::Esc => {
          logger.remove_refresh_item(LoggerRefreshItemKind::Selection);
          bail!("Selection cancelled.");
        }
        _ => {}
      }
    } else {
      // cause a refresh anyway
    }
  }
  logger.remove_refresh_item(LoggerRefreshItemKind::Selection);

  logger.log_text_items(&render_complete(&data), context_name);

  // return the selected indexes
  let mut result = Vec::new();
  for (i, (is_selected, _)) in data.items.iter().enumerate() {
    if *is_selected {
      result.push(i);
    }
  }
  Ok(result)
}

/// The indexes into `data.items` that match the current filter, in order.
fn visible_indexes(data: &MultiSelectData) -> Vec<usize> {
  if data.filter.is_empty() {
    return (0..data.items.len()).collect();
  }
  let filter = data.filter.to_lowercase();
  data
    .items
    .iter()
    .enumerate()
    .filter(|(_, (_, text))| text.to_lowercase().contains(&filter))
    .map(|(i, _)| i)
    .collect()
}

/// The maximum number of items to show at once, based on the terminal height.
/// When the terminal size is unknown (ex. not a tty) everything is shown.
fn max_visible_rows(data: &MultiSelectData) -> usize {
  match get_terminal_size() {
    Some(size) => {
      // reserve rows for: the prompt, the filter line, both scroll indicators,
      // and a little breathing room so the list doesn't fill the entire screen
      let reserved = 1 + usize::from(!data.filter.is_empty()) + 2 + 1;
      (size.rows as usize).saturating_sub(reserved).max(1)
    }
    None => data.items.len().max(1),
  }
}

/// Adjusts the scroll offset so the active item stays within the visible window.
fn update_scroll_offset(data: &mut MultiSelectData, visible_len: usize, max_visible_rows: usize) {
  if data.active_index < data.scroll_offset {
    data.scroll_offset = data.active_index;
  } else if data.active_index >= data.scroll_offset + max_visible_rows {
    data.scroll_offset = data.active_index + 1 - max_visible_rows;
  }
  let max_scroll = visible_len.saturating_sub(max_visible_rows);
  if data.scroll_offset > max_scroll {
    data.scroll_offset = max_scroll;
  }
}

fn render_multi_select(data: &MultiSelectData, visible: &[usize], max_visible_rows: usize) -> Vec<LoggerTextItem> {
  let mut result = vec![LoggerTextItem::Text(data.prompt.to_string())];

  if !data.filter.is_empty() {
    result.push(LoggerTextItem::Text(format!("  filter: {}", data.filter)));
  }

  if visible.is_empty() {
    result.push(LoggerTextItem::Text("  (no matching plugins)".to_string()));
    return result;
  }

  let end = (data.scroll_offset + max_visible_rows).min(visible.len());
  if data.scroll_offset > 0 {
    result.push(LoggerTextItem::Text(format!("  ...{} more above", data.scroll_offset)));
  }

  for (visible_pos, &item_index) in visible.iter().enumerate().take(end).skip(data.scroll_offset) {
    let (is_selected, item_text) = &data.items[item_index];
    let mut text = String::new();
    text.push_str(if visible_pos == data.active_index { ">" } else { " " });
    text.push_str(" [");
    text.push_str(if *is_selected { "x" } else { " " });
    text.push_str("] ");
    text.push_str(item_text);

    result.push(LoggerTextItem::HangingText {
      text,
      indent: 7 + data.item_hanging_indent,
    });
  }

  if end < visible.len() {
    result.push(LoggerTextItem::Text(format!("  ...{} more below", visible.len() - end)));
  }

  result
}

fn render_complete(data: &MultiSelectData) -> Vec<LoggerTextItem> {
  let mut result = Vec::new();
  if data.items.iter().any(|(is_selected, _)| *is_selected) {
    result.push(LoggerTextItem::Text(data.prompt.to_string()));
    for (is_selected, item_text) in data.items.iter() {
      if *is_selected {
        result.push(LoggerTextItem::HangingText {
          text: format!(" * {}", item_text),
          indent: 3 + data.item_hanging_indent,
        });
      }
    }
  }
  result
}
