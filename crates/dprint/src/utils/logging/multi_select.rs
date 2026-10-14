use anyhow::Result;
use anyhow::bail;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use deno_terminal::colors;

use crate::environment::Environment;
use crate::utils::terminal::TerminalSize;

use super::LoggerTextItem;
use super::PromptTerminal;
use super::fits_terminal_height;
use super::terminal_too_small_text_items;

/// An item in a multi-select prompt.
pub struct MultiSelectItem {
  pub text: String,
  /// Secondary text shown dimmed after the item's text (ex. the file
  /// extensions a plugin formats). Empty when there's nothing to show.
  pub detail: Vec<MultiSelectDetailPart>,
  /// Whether the item starts out selected.
  pub is_selected: bool,
  /// Whether the user can toggle the item. A non-selectable item is shown for
  /// context (ex. a plugin that's already in the config file) and is never
  /// part of the result.
  pub is_selectable: bool,
  /// Whether the item is marked, which is shown with a star (ex. a plugin the
  /// user prefers). It's only there to be seen.
  pub is_marked: bool,
  /// Identifies the item to whoever is showing the prompt, since the user may
  /// move the items around when the prompt allows it.
  pub id: usize,
  /// Where the item goes among the items that are selected the way it is
  /// when the prompt keeps the selected items first, lowest first.
  pub rank: usize,
}

/// A piece of the detail shown after a multi-select item's text.
pub struct MultiSelectDetailPart {
  pub text: String,
  /// Whether the part stands out from the rest of the detail (ex. a file
  /// extension that was found in the current directory).
  pub is_highlighted: bool,
}

impl MultiSelectDetailPart {
  pub fn new(text: impl Into<String>) -> Self {
    MultiSelectDetailPart {
      text: text.into(),
      is_highlighted: false,
    }
  }
}

impl MultiSelectItem {
  pub fn new(text: String, is_selected: bool) -> Self {
    MultiSelectItem {
      text,
      detail: Vec::new(),
      is_selected,
      is_selectable: true,
      is_marked: false,
      id: 0,
      rank: 0,
    }
  }

  /// An item shown as selected that the user can't toggle.
  pub fn non_selectable(text: String) -> Self {
    MultiSelectItem {
      text,
      detail: Vec::new(),
      is_selected: true,
      is_selectable: false,
      is_marked: false,
      id: 0,
      rank: 0,
    }
  }

  pub fn with_detail(mut self, detail: Vec<MultiSelectDetailPart>) -> Self {
    self.detail = detail;
    self
  }

  pub fn with_id(mut self, id: usize) -> Self {
    self.id = id;
    self
  }

  pub fn with_rank(mut self, rank: usize) -> Self {
    self.rank = rank;
    self
  }

  pub fn with_marked(mut self, is_marked: bool) -> Self {
    self.is_marked = is_marked;
    self
  }

  /// The item's text followed by its detail.
  pub fn full_text(&self) -> String {
    let mut text = self.text.clone();
    if !self.detail.is_empty() {
      text.push(' ');
      for part in &self.detail {
        text.push_str(&part.text);
      }
    }
    text
  }

  /// The item as it's shown in the terminal: its detail is dimmed apart from
  /// the highlighted parts, and so is the text of an item that can't be
  /// toggled so it's clear it's only there for context.
  fn render(&self) -> String {
    let mut text = if self.is_selectable {
      self.text.clone()
    } else {
      colors::gray(&self.text).to_string()
    };
    if self.is_marked {
      text.push(' ');
      text.push_str(&colors::yellow(MARK_TEXT).to_string());
    }
    if !self.detail.is_empty() {
      text.push(' ');
      for part in &self.detail {
        if part.is_highlighted {
          text.push_str(&colors::cyan(&part.text).to_string());
        } else {
          text.push_str(&colors::gray(&part.text).to_string());
        }
      }
    }
    text
  }
}

/// What the user is allowed to do in a multi-select prompt in addition to
/// toggling its items.
#[derive(Default, Clone, Copy)]
pub struct MultiSelectFeatures {
  /// Shift, alt, or ctrl with the up and down arrows moves the active item.
  pub can_move: bool,
  /// The right arrow asks to open the active item, which hands control back
  /// to the caller with the prompt's state kept so it can be shown again.
  pub can_open: bool,
  /// Keeps the selected items ahead of the others, which is for when the
  /// order of the selected items is part of what's being asked for. Selecting
  /// an item moves it up to where its rank puts it among the selected ones,
  /// unselecting it moves it back down, and only selected items can be moved.
  /// Items that can't be selected count as selected ones that stay in the
  /// order they're in: they're expected to go first, and a selected item can
  /// be moved in between them.
  pub selected_first: bool,
}

/// The state of a multi-select prompt. It's owned by the caller so the prompt
/// can be left to show something else (ex. the details of an item) and then
/// be shown again as it was.
pub struct MultiSelectState {
  pub prompt: String,
  /// Shown dimmed under the prompt (ex. the keys that do something).
  pub help: Option<String>,
  pub item_hanging_indent: u16,
  pub items: Vec<MultiSelectItem>,
  pub features: MultiSelectFeatures,
  /// Text typed by the user to narrow down the visible items.
  filter: String,
  /// Index into the currently visible (filtered) items.
  active_index: usize,
  /// First visible (filtered) item, used to scroll long lists.
  scroll_offset: usize,
  /// How many items were shown at once the last time the prompt was drawn,
  /// which is how far page up and page down go.
  page_size: usize,
}

impl MultiSelectState {
  pub fn new(prompt: impl Into<String>, items: Vec<MultiSelectItem>) -> Self {
    // start on something that can be toggled
    let active_index = items.iter().position(|item| item.is_selectable).unwrap_or(0);
    MultiSelectState {
      prompt: prompt.into(),
      help: None,
      item_hanging_indent: 0,
      items,
      features: Default::default(),
      filter: String::new(),
      active_index,
      scroll_offset: 0,
      page_size: 1,
    }
  }

  /// Selects or unselects the item at this index in the items, moving it to
  /// where it goes when the prompt keeps the selected items first. That's
  /// under the items that can't be unselected, since being above one of those
  /// is something only the user moving an item there asks for. The item stays
  /// the active one when it was.
  pub fn set_item_selected(&mut self, item_index: usize, is_selected: bool) {
    let was_active = visible_indexes(self).get(self.active_index) == Some(&item_index);
    self.items[item_index].is_selected = is_selected;
    if !self.features.selected_first {
      return;
    }
    let item = self.items.remove(item_index);
    let goes_before = |other: &MultiSelectItem| {
      if item.is_selected {
        !other.is_selected || other.rank > item.rank
      } else {
        !other.is_selected && other.rank > item.rank
      }
    };
    let start_index = self.items.iter().rposition(|other| !other.is_selectable).map(|index| index + 1).unwrap_or(0);
    let new_index = self.items[start_index..]
      .iter()
      .position(goes_before)
      .map(|index| start_index + index)
      .unwrap_or(self.items.len());
    self.items.insert(new_index, item);
    if was_active && let Some(active_index) = visible_indexes(self).iter().position(|&index| index == new_index) {
      self.active_index = active_index;
    }
  }

  /// The ids of the selected items in the order they're shown in.
  /// Non-selectable items are never part of this.
  pub fn selected_ids(&self) -> Vec<usize> {
    self
      .items
      .iter()
      .filter(|item| item.is_selected && item.is_selectable)
      .map(|item| item.id)
      .collect()
  }
}

/// What the user did to leave a multi-select prompt.
#[derive(Debug, PartialEq, Eq)]
pub enum MultiSelectAction {
  /// The user finished selecting.
  Done,
  /// The user asked to open the item at this index in the state's items. The
  /// prompt should be shown again with the same state afterwards.
  Open(usize),
}

/// Shows a multi-select prompt in the environment's terminal until the user
/// finishes or asks to open an item, in which case this should be called
/// again with the same state once what was opened is done with. The
/// selection is read from the state.
pub fn show_multi_select(environment: &impl Environment, context_name: &str, state: &mut MultiSelectState) -> Result<MultiSelectAction> {
  let terminal = environment.prompt_terminal();
  run_multi_select(terminal.as_ref(), context_name, state)
}

fn run_multi_select(terminal: &dyn PromptTerminal, context_name: &str, state: &mut MultiSelectState) -> Result<MultiSelectAction> {
  loop {
    let visible = visible_indexes(state);
    // keep the active index and scrolling within the visible items
    if state.active_index >= visible.len() {
      state.active_index = visible.len().saturating_sub(1);
    }
    let terminal_size = terminal.size();
    let mut max_visible_rows = max_visible_rows(state, terminal_size);
    let mut is_too_small = false;
    let text_items = loop {
      update_scroll_offset(state, visible.len(), max_visible_rows);
      let text_items = render_multi_select(state, &visible, max_visible_rows);
      // show less when lines wrapping would push the prompt off the screen
      if fits_terminal_height(&text_items, terminal_size) {
        break text_items;
      }
      if max_visible_rows == 1 {
        is_too_small = true;
        break terminal_too_small_text_items();
      }
      max_visible_rows -= 1;
    };
    state.page_size = max_visible_rows;
    terminal.draw(text_items);

    if let Event::Key(key_event) = terminal.read_key()? {
      // what a key does can't be seen, so the only thing to do is cancel
      if is_too_small && key_event.code != KeyCode::Esc {
        continue;
      }
      match handle_key(state, &key_event) {
        KeyOutcome::Continue => {}
        KeyOutcome::Done => break,
        // what gets opened is shown in place of the prompt
        KeyOutcome::Open(item_index) => return Ok(MultiSelectAction::Open(item_index)),
        KeyOutcome::Cancel => {
          terminal.clear();
          bail!("Selection cancelled.");
        }
      }
    } else {
      // cause a refresh anyway
    }
  }
  terminal.clear();

  terminal.log(&render_complete(state), context_name);

  Ok(MultiSelectAction::Done)
}

/// The text shown after a marked item.
const MARK_TEXT: &str = "\u{2605}";

#[derive(Debug, PartialEq, Eq)]
enum KeyOutcome {
  Continue,
  Done,
  Cancel,
  Open(usize),
}

fn handle_key(state: &mut MultiSelectState, key_event: &KeyEvent) -> KeyOutcome {
  // ctrl+c cancels
  if key_event.modifiers.contains(KeyModifiers::CONTROL) && matches!(key_event.code, KeyCode::Char('c')) {
    return KeyOutcome::Cancel;
  }
  let visible = visible_indexes(state);
  // not every terminal reports shift with the arrow keys, so alt and ctrl work too
  let has_move_modifier = key_event.modifiers.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT | KeyModifiers::CONTROL);
  match &key_event.code {
    KeyCode::Up | KeyCode::Down if has_move_modifier => {
      move_active_item(state, matches!(key_event.code, KeyCode::Up));
    }
    KeyCode::Up => {
      if !visible.is_empty() {
        state.active_index = (state.active_index + visible.len() - 1) % visible.len();
      }
    }
    KeyCode::Down => {
      if !visible.is_empty() {
        state.active_index = (state.active_index + 1) % visible.len();
      }
    }
    KeyCode::PageUp => {
      state.active_index = state.active_index.saturating_sub(state.page_size.max(1));
    }
    KeyCode::PageDown => {
      state.active_index = state.active_index.saturating_add(state.page_size.max(1)).min(visible.len().saturating_sub(1));
    }
    KeyCode::Char(' ') => {
      // toggle the active item's selection
      if let Some(&item_index) = visible.get(state.active_index) {
        let item = &state.items[item_index];
        if item.is_selectable {
          state.set_item_selected(item_index, !item.is_selected);
        }
      }
    }
    KeyCode::Right => {
      // an item that can't be selected is only there to be seen
      if state.features.can_open
        && let Some(&item_index) = visible.get(state.active_index)
        && state.items[item_index].is_selectable
      {
        return KeyOutcome::Open(item_index);
      }
    }
    KeyCode::Backspace => {
      if state.filter.pop().is_some() {
        state.active_index = 0;
        state.scroll_offset = 0;
      }
    }
    KeyCode::Char(c) => {
      // any other printable character narrows down the list
      if !key_event.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) {
        state.filter.push(*c);
        state.active_index = 0;
        state.scroll_offset = 0;
      }
    }
    KeyCode::Enter => return KeyOutcome::Done,
    KeyCode::Esc => return KeyOutcome::Cancel,
    _ => {}
  }
  KeyOutcome::Continue
}

/// Moves the active item up or down a position, keeping it active. Items
/// aren't moved while filtering because a position in a filtered list doesn't
/// say where the item ends up among the items that are hidden.
fn move_active_item(state: &mut MultiSelectState, is_up: bool) {
  if !state.features.can_move || !state.filter.is_empty() || state.active_index >= state.items.len() {
    return;
  }
  let index = state.active_index;
  let new_index = if is_up {
    match index.checked_sub(1) {
      Some(new_index) => new_index,
      None => return,
    }
  } else if index + 1 < state.items.len() {
    index + 1
  } else {
    return;
  };
  // the order only says something about the selected items, and the ones
  // that can't be selected aren't the user's to move
  if state.features.selected_first {
    let item = &state.items[index];
    if !item.is_selected || !item.is_selectable || !state.items[new_index].is_selected {
      return;
    }
  }
  state.items.swap(index, new_index);
  state.active_index = new_index;
}

/// The indexes into `data.items` that match the current filter, in order.
fn visible_indexes(data: &MultiSelectState) -> Vec<usize> {
  if data.filter.is_empty() {
    return (0..data.items.len()).collect();
  }
  let filter = data.filter.to_lowercase();
  data
    .items
    .iter()
    .enumerate()
    .filter(|(_, item)| item.full_text().to_lowercase().contains(&filter))
    .map(|(i, _)| i)
    .collect()
}

/// The maximum number of items to show at once, based on the terminal height.
/// When the terminal size is unknown (ex. not a tty) everything is shown.
fn max_visible_rows(data: &MultiSelectState, terminal_size: Option<TerminalSize>) -> usize {
  match terminal_size {
    Some(size) => {
      // reserve rows for: the prompt, the help line, the filter line, both scroll indicators,
      // and a little breathing room so the list doesn't fill the entire screen
      let reserved = 1 + usize::from(data.help.is_some()) + usize::from(!data.filter.is_empty()) + 2 + 1;
      (size.rows as usize).saturating_sub(reserved).max(1)
    }
    None => data.items.len().max(1),
  }
}

/// Adjusts the scroll offset so the active item stays within the visible window.
fn update_scroll_offset(data: &mut MultiSelectState, visible_len: usize, max_visible_rows: usize) {
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

fn render_multi_select(data: &MultiSelectState, visible: &[usize], max_visible_rows: usize) -> Vec<LoggerTextItem> {
  let mut result = vec![LoggerTextItem::Text(data.prompt.to_string())];

  if let Some(help) = &data.help {
    result.push(LoggerTextItem::HangingText {
      text: format!("  {}", colors::gray(help)),
      indent: 2,
    });
  }

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
    text.push_str(&item.render());

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
fn render_complete(data: &MultiSelectState) -> Vec<LoggerTextItem> {
  let mut result = Vec::new();
  let is_chosen = |item: &MultiSelectItem| item.is_selected && item.is_selectable;
  if data.items.iter().any(is_chosen) {
    result.push(LoggerTextItem::Text(data.prompt.to_string()));
    for item in data.items.iter().filter(|item| is_chosen(item)) {
      result.push(LoggerTextItem::HangingText {
        text: format!(" * {}", item.render()),
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

  fn build_data(items: &[(bool, String)]) -> MultiSelectState {
    build_data_with_items(items.iter().map(|(selected, text)| MultiSelectItem::new(text.clone(), *selected)).collect())
  }

  fn build_data_with_items(items: Vec<MultiSelectItem>) -> MultiSelectState {
    MultiSelectState::new("Select:", items)
  }

  fn press(data: &mut MultiSelectState, code: KeyCode) -> KeyOutcome {
    press_with_modifiers(data, code, KeyModifiers::NONE)
  }

  fn press_with_modifiers(data: &mut MultiSelectState, code: KeyCode, modifiers: KeyModifiers) -> KeyOutcome {
    handle_key(data, &KeyEvent::new(code, modifiers))
  }

  fn item_texts(data: &MultiSelectState) -> Vec<&str> {
    data.items.iter().map(|item| item.text.as_str()).collect()
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
  fn render_dims_item_details_apart_from_highlighted_parts() {
    let data = build_data_with_items(vec![
      MultiSelectItem::new("alpha".to_string(), false).with_detail(vec![
        MultiSelectDetailPart::new("("),
        MultiSelectDetailPart {
          text: ".a".to_string(),
          is_highlighted: true,
        },
        MultiSelectDetailPart::new(", .b)"),
      ]),
      MultiSelectItem::non_selectable("beta".to_string()).with_detail(vec![MultiSelectDetailPart::new("(.c)")]),
    ]);
    assert_eq!(data.items[0].full_text(), "alpha (.a, .b)");
    let visible = visible_indexes(&data);
    assert_eq!(
      rendered_lines(&render_multi_select(&data, &visible, 10)),
      vec![
        "Select:".to_string(),
        format!("> [ ] alpha {}{}{}", colors::gray("("), colors::cyan(".a"), colors::gray(", .b)")),
        format!("  [x] {} {}", colors::gray("beta"), colors::gray("(.c)")),
      ]
    );
  }

  #[test]
  fn visible_indexes_filters_on_item_details() {
    let mut data = build_data_with_items(vec![
      MultiSelectItem::new("alpha".to_string(), false).with_detail(vec![MultiSelectDetailPart::new("(.ts)")]),
      MultiSelectItem::new("beta".to_string(), false).with_detail(vec![MultiSelectDetailPart::new("(.json)")]),
    ]);
    data.filter = ".json".to_string();
    assert_eq!(visible_indexes(&data), vec![1]);
    data.filter = "alpha (".to_string();
    assert_eq!(visible_indexes(&data), vec![0]);
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

  fn selected_first_data() -> MultiSelectState {
    let mut data = build_data_with_items(vec![
      MultiSelectItem::non_selectable("locked".to_string()),
      MultiSelectItem::new("b".to_string(), true).with_rank(1),
      MultiSelectItem::new("d".to_string(), true).with_rank(3),
      MultiSelectItem::new("a".to_string(), false).with_rank(0),
      MultiSelectItem::new("c".to_string(), false).with_rank(2),
      MultiSelectItem::new("e".to_string(), false).with_rank(4),
    ]);
    data.features.selected_first = true;
    data.features.can_move = true;
    data
  }

  #[test]
  fn starts_on_the_first_item_that_can_be_toggled() {
    let data = selected_first_data();
    assert_eq!(data.active_index, 1);
  }

  #[test]
  fn selecting_moves_the_item_to_its_rank_among_the_selected_items() {
    let mut data = selected_first_data();
    // `c` goes between `b` and `d` and stays the active item
    data.active_index = 4;
    press(&mut data, KeyCode::Char(' '));
    assert_eq!(item_texts(&data), vec!["locked", "b", "c", "d", "a", "e"]);
    assert_eq!(data.active_index, 2);
    assert!(data.items[2].is_selected);

    // `e` goes last among the selected items and `a` first
    data.active_index = 5;
    press(&mut data, KeyCode::Char(' '));
    assert_eq!(item_texts(&data), vec!["locked", "b", "c", "d", "e", "a"]);
    assert_eq!(data.active_index, 4);
    data.active_index = 5;
    press(&mut data, KeyCode::Char(' '));
    assert_eq!(item_texts(&data), vec!["locked", "a", "b", "c", "d", "e"]);
    assert_eq!(data.active_index, 1);

    // unselecting goes back down to its rank among the rest
    data.active_index = 3;
    press(&mut data, KeyCode::Char(' '));
    assert_eq!(item_texts(&data), vec!["locked", "a", "b", "d", "e", "c"]);
    assert_eq!(data.active_index, 5);
    data.active_index = 1;
    press(&mut data, KeyCode::Char(' '));
    assert_eq!(item_texts(&data), vec!["locked", "b", "d", "e", "a", "c"]);
    assert_eq!(data.active_index, 4);
  }

  #[test]
  fn selecting_while_filtering_keeps_the_item_active() {
    let mut data = selected_first_data();
    data.filter = "e".to_string();
    assert_eq!(visible_indexes(&data), vec![0, 5]);
    data.active_index = 1;
    press(&mut data, KeyCode::Char(' '));
    assert_eq!(item_texts(&data), vec!["locked", "b", "d", "e", "a", "c"]);
    assert_eq!(visible_indexes(&data), vec![0, 3]);
    assert_eq!(data.active_index, 1);
  }

  #[test]
  fn only_selected_items_move_when_they_are_kept_first() {
    let mut data = selected_first_data();
    let shift = KeyModifiers::SHIFT;
    // above what can't be selected, which is as good as selected
    press_with_modifiers(&mut data, KeyCode::Up, shift);
    assert_eq!(item_texts(&data), vec!["b", "locked", "d", "a", "c", "e"]);
    press_with_modifiers(&mut data, KeyCode::Down, shift);
    press_with_modifiers(&mut data, KeyCode::Down, shift);
    assert_eq!(item_texts(&data), vec!["locked", "d", "b", "a", "c", "e"]);
    // not below what isn't selected
    press_with_modifiers(&mut data, KeyCode::Down, shift);
    assert_eq!(item_texts(&data), vec!["locked", "d", "b", "a", "c", "e"]);
    // what isn't selected stays where it is
    data.active_index = 4;
    press_with_modifiers(&mut data, KeyCode::Up, shift);
    assert_eq!(item_texts(&data), vec!["locked", "d", "b", "a", "c", "e"]);
    // and so does what can't be selected
    data.active_index = 0;
    press_with_modifiers(&mut data, KeyCode::Down, shift);
    assert_eq!(item_texts(&data), vec!["locked", "d", "b", "a", "c", "e"]);
  }

  #[test]
  fn selecting_does_not_move_the_item_above_what_can_not_be_unselected() {
    let mut data = selected_first_data();
    // `d` gets moved above it
    data.active_index = 2;
    press_with_modifiers(&mut data, KeyCode::Up, KeyModifiers::SHIFT);
    press_with_modifiers(&mut data, KeyCode::Up, KeyModifiers::SHIFT);
    assert_eq!(item_texts(&data), vec!["d", "locked", "b", "a", "c", "e"]);
    // which isn't asking for `a` or `c` to be above it, even though they go
    // ahead of `d` otherwise
    data.active_index = 3;
    press(&mut data, KeyCode::Char(' '));
    assert_eq!(item_texts(&data), vec!["d", "locked", "a", "b", "c", "e"]);
    data.active_index = 4;
    press(&mut data, KeyCode::Char(' '));
    assert_eq!(item_texts(&data), vec!["d", "locked", "a", "b", "c", "e"]);
    assert!(data.items[4].is_selected);

    // unselecting what's above it goes back down
    data.active_index = 0;
    press(&mut data, KeyCode::Char(' '));
    assert_eq!(item_texts(&data), vec!["locked", "a", "b", "c", "d", "e"]);
    assert_eq!(data.active_index, 4);
  }

  #[test]
  fn page_keys_move_by_what_was_shown_without_wrapping() {
    let items = (0..10).map(|i| (false, i.to_string())).collect::<Vec<_>>();
    let mut data = build_data(&items);
    data.page_size = 4;
    press(&mut data, KeyCode::PageDown);
    assert_eq!(data.active_index, 4);
    press(&mut data, KeyCode::PageDown);
    press(&mut data, KeyCode::PageDown);
    assert_eq!(data.active_index, 9);
    press(&mut data, KeyCode::PageUp);
    assert_eq!(data.active_index, 5);
    press(&mut data, KeyCode::PageUp);
    press(&mut data, KeyCode::PageUp);
    assert_eq!(data.active_index, 0);
  }

  #[test]
  fn render_shows_a_star_after_a_marked_item() {
    let data = build_data_with_items(vec![MultiSelectItem::new("alpha".to_string(), true).with_marked(true)]);
    let visible = visible_indexes(&data);
    assert_eq!(
      rendered_lines(&render_multi_select(&data, &visible, 10)),
      vec!["Select:".to_string(), format!("> [x] alpha {}", colors::yellow(MARK_TEXT))]
    );
  }

  #[test]
  fn modified_arrows_move_the_active_item_when_allowed() {
    let mut data = build_data(&[(false, "a".to_string()), (false, "b".to_string()), (false, "c".to_string())]);
    press_with_modifiers(&mut data, KeyCode::Down, KeyModifiers::SHIFT);
    assert_eq!(item_texts(&data), vec!["a", "b", "c"]);

    data.features.can_move = true;
    press_with_modifiers(&mut data, KeyCode::Down, KeyModifiers::SHIFT);
    assert_eq!(item_texts(&data), vec!["b", "a", "c"]);
    assert_eq!(data.active_index, 1);
    press_with_modifiers(&mut data, KeyCode::Down, KeyModifiers::ALT);
    assert_eq!(item_texts(&data), vec!["b", "c", "a"]);
    // already at the bottom
    press_with_modifiers(&mut data, KeyCode::Down, KeyModifiers::CONTROL);
    assert_eq!(item_texts(&data), vec!["b", "c", "a"]);
    press_with_modifiers(&mut data, KeyCode::Up, KeyModifiers::SHIFT);
    assert_eq!(item_texts(&data), vec!["b", "a", "c"]);
    assert_eq!(data.active_index, 1);

    // not while filtering
    data.filter = "a".to_string();
    data.active_index = 0;
    press_with_modifiers(&mut data, KeyCode::Down, KeyModifiers::SHIFT);
    assert_eq!(item_texts(&data), vec!["b", "a", "c"]);
  }

  #[test]
  fn right_arrow_opens_the_active_item_when_allowed() {
    let mut data = build_data(&[(false, "alpha".to_string()), (false, "beta".to_string())]);
    assert_eq!(press(&mut data, KeyCode::Right), KeyOutcome::Continue);
    data.features.can_open = true;
    data.filter = "beta".to_string();
    // the index is into the items rather than the filtered ones
    assert_eq!(press(&mut data, KeyCode::Right), KeyOutcome::Open(1));
    // what can't be selected can't be opened
    data.items[1].is_selectable = false;
    assert_eq!(press(&mut data, KeyCode::Right), KeyOutcome::Continue);
  }

  #[test]
  fn selected_ids_follow_the_display_order() {
    let mut data = build_data_with_items(vec![
      MultiSelectItem::new("alpha".to_string(), true).with_id(5),
      MultiSelectItem::new("beta".to_string(), true).with_id(2),
      MultiSelectItem::non_selectable("gamma".to_string()).with_id(9),
    ]);
    data.features.can_move = true;
    assert_eq!(data.selected_ids(), vec![5, 2]);
    press_with_modifiers(&mut data, KeyCode::Down, KeyModifiers::SHIFT);
    assert_eq!(data.selected_ids(), vec![2, 5]);
  }

  #[test]
  fn render_shows_help_under_the_prompt() {
    let mut data = build_data(&[(false, "alpha".to_string())]);
    data.help = Some("space: toggle".to_string());
    let visible = visible_indexes(&data);
    assert_eq!(
      rendered_lines(&render_multi_select(&data, &visible, 10)),
      vec!["Select:".to_string(), format!("  {}", colors::gray("space: toggle")), "> [ ] alpha".to_string()]
    );
  }
}
