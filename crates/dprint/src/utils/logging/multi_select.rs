use anyhow::Result;
use anyhow::bail;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyModifiers;
use deno_terminal::colors;

use crate::utils::terminal::get_terminal_size;
use crate::utils::terminal::read_terminal_key_press;

use super::Logger;
use super::LoggerRefreshItemKind;
use super::LoggerTextItem;

/// An item in a multi-select prompt.
pub struct MultiSelectItem {
  pub text: String,
  /// Whether the item starts out selected.
  pub is_selected: bool,
  /// Whether the user can toggle the item. A non-selectable item is shown for
  /// context (ex. a plugin that's already in the config file) and is never
  /// part of the result.
  pub is_selectable: bool,
}

impl MultiSelectItem {
  pub fn new(text: String, is_selected: bool) -> Self {
    MultiSelectItem {
      text,
      is_selected,
      is_selectable: true,
    }
  }

  /// An item shown as selected that the user can't toggle.
  pub fn non_selectable(text: String) -> Self {
    MultiSelectItem {
      text,
      is_selected: true,
      is_selectable: false,
    }
  }
}

struct MultiSelectData<'a> {
  prompt: &'a str,
  item_hanging_indent: u16,
  items: Vec<MultiSelectItem>,
  /// Text typed by the user to narrow down the visible items.
  filter: String,
  /// Index into the currently visible (filtered) items.
  active_index: usize,
  /// First visible (filtered) item, used to scroll long lists.
  scroll_offset: usize,
}

/// Shows a multi-select prompt, returning the indexes of the selected items.
/// Non-selectable items are shown but never returned.
pub fn show_multi_select(logger: &Logger, context_name: &str, prompt: &str, item_hanging_indent: u16, items: Vec<MultiSelectItem>) -> Result<Vec<usize>> {
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
            let item = &mut data.items[item_index];
            if item.is_selectable {
              item.is_selected = !item.is_selected;
            }
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
  for (i, item) in data.items.iter().enumerate() {
    if item.is_selected && item.is_selectable {
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
    .filter(|(_, item)| item.text.to_lowercase().contains(&filter))
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
    let item = &data.items[item_index];
    let mut text = String::new();
    text.push_str(if visible_pos == data.active_index { ">" } else { " " });
    text.push_str(" [");
    text.push_str(if item.is_selected { "x" } else { " " });
    text.push_str("] ");
    // dim the items that can't be toggled so it's clear they're only context
    if item.is_selectable {
      text.push_str(&item.text);
    } else {
      text.push_str(&colors::gray(&item.text).to_string());
    }

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

/// The prompt's final state, logged once it's done: only what the user chose,
/// since the non-selectable items were shown for context.
fn render_complete(data: &MultiSelectData) -> Vec<LoggerTextItem> {
  let mut result = Vec::new();
  let is_chosen = |item: &MultiSelectItem| item.is_selected && item.is_selectable;
  if data.items.iter().any(is_chosen) {
    result.push(LoggerTextItem::Text(data.prompt.to_string()));
    for item in data.items.iter().filter(|item| is_chosen(item)) {
      result.push(LoggerTextItem::HangingText {
        text: format!(" * {}", item.text),
        indent: 3 + data.item_hanging_indent,
      });
    }
  }
  result
}

#[cfg(test)]
mod test {
  use super::*;
  use pretty_assertions::assert_eq;

  fn build_data(items: &[(bool, String)]) -> MultiSelectData<'static> {
    build_data_with_items(items.iter().map(|(selected, text)| MultiSelectItem::new(text.clone(), *selected)).collect())
  }

  fn build_data_with_items(items: Vec<MultiSelectItem>) -> MultiSelectData<'static> {
    MultiSelectData {
      prompt: "Select:",
      item_hanging_indent: 0,
      items,
      filter: String::new(),
      active_index: 0,
      scroll_offset: 0,
    }
  }

  fn rendered_lines(items: &[LoggerTextItem]) -> Vec<String> {
    items
      .iter()
      .map(|item| match item {
        LoggerTextItem::Text(text) => text.clone(),
        LoggerTextItem::HangingText { text, .. } => text.clone(),
      })
      .collect()
  }

  #[test]
  fn visible_indexes_returns_all_when_no_filter() {
    let items = vec![(false, "alpha".to_string()), (false, "beta".to_string())];
    let data = build_data(&items);
    assert_eq!(visible_indexes(&data), vec![0, 1]);
  }

  #[test]
  fn visible_indexes_filters_case_insensitively() {
    let items = vec![(false, "TypeScript".to_string()), (false, "JSON".to_string()), (false, "Markdown".to_string())];
    let mut data = build_data(&items);
    data.filter = "json".to_string();
    assert_eq!(visible_indexes(&data), vec![1]);
    data.filter = "s".to_string(); // matches "TypeScript" and "JSON"
    assert_eq!(visible_indexes(&data), vec![0, 1]);
    data.filter = "nope".to_string();
    assert_eq!(visible_indexes(&data), Vec::<usize>::new());
  }

  #[test]
  fn update_scroll_offset_scrolls_to_keep_active_visible() {
    let items = (0..10).map(|i| (false, format!("item{i}"))).collect::<Vec<_>>();
    let mut data = build_data(&items);

    // active below the window scrolls down so it's the last visible row
    data.active_index = 7;
    update_scroll_offset(&mut data, 10, 3);
    assert_eq!(data.scroll_offset, 5);

    // active above the window scrolls up to the active row
    data.active_index = 2;
    update_scroll_offset(&mut data, 10, 3);
    assert_eq!(data.scroll_offset, 2);

    // active already visible doesn't move the window
    data.active_index = 3;
    update_scroll_offset(&mut data, 10, 3);
    assert_eq!(data.scroll_offset, 2);
  }

  #[test]
  fn update_scroll_offset_clamps_to_end() {
    let items = (0..5).map(|i| (false, format!("item{i}"))).collect::<Vec<_>>();
    let mut data = build_data(&items);
    data.scroll_offset = 4;
    data.active_index = 4;
    update_scroll_offset(&mut data, 5, 3);
    // max scroll is 5 - 3 = 2
    assert_eq!(data.scroll_offset, 2);
  }

  #[test]
  fn render_marks_active_and_selected() {
    let items = vec![(true, "alpha".to_string()), (false, "beta".to_string())];
    let mut data = build_data(&items);
    data.active_index = 1;
    let visible = visible_indexes(&data);
    assert_eq!(
      rendered_lines(&render_multi_select(&data, &visible, 10)),
      vec!["Select:", "  [x] alpha", "> [ ] beta"]
    );
  }

  #[test]
  fn render_dims_non_selectable_items() {
    let data = build_data_with_items(vec![
      MultiSelectItem::new("alpha".to_string(), false),
      MultiSelectItem::non_selectable("beta".to_string()),
    ]);
    let visible = visible_indexes(&data);
    assert_eq!(
      rendered_lines(&render_multi_select(&data, &visible, 10)),
      vec!["Select:".to_string(), "> [ ] alpha".to_string(), format!("  [x] {}", colors::gray("beta")),]
    );
  }

  #[test]
  fn render_complete_only_shows_what_was_chosen() {
    let data = build_data_with_items(vec![
      MultiSelectItem::new("alpha".to_string(), true),
      MultiSelectItem::new("beta".to_string(), false),
      // shown while selecting, but not something the user chose
      MultiSelectItem::non_selectable("gamma".to_string()),
    ]);
    assert_eq!(rendered_lines(&render_complete(&data)), vec!["Select:", " * alpha"]);
  }

  #[test]
  fn render_shows_filter_and_scroll_indicators() {
    let items = (0..10).map(|i| (false, format!("item{i}"))).collect::<Vec<_>>();
    let mut data = build_data(&items);
    data.filter = "item".to_string();
    data.active_index = 5;
    data.scroll_offset = 4;
    let visible = visible_indexes(&data);
    assert_eq!(
      rendered_lines(&render_multi_select(&data, &visible, 3)),
      vec![
        "Select:",
        "  filter: item",
        "  ...4 more above",
        "  [ ] item4",
        "> [ ] item5",
        "  [ ] item6",
        "  ...3 more below",
      ]
    );
  }

  #[test]
  fn render_shows_message_when_no_matches() {
    let items = vec![(false, "alpha".to_string())];
    let mut data = build_data(&items);
    data.filter = "nope".to_string();
    let visible = visible_indexes(&data);
    assert_eq!(
      rendered_lines(&render_multi_select(&data, &visible, 10)),
      vec!["Select:", "  filter: nope", "  (no matching plugins)"]
    );
  }
}
