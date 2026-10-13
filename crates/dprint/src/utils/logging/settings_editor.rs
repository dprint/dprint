use anyhow::Result;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use deno_terminal::colors;
use dprint_core::async_runtime::LocalBoxFuture;
use dprint_core::async_runtime::future;
use dprint_core::async_runtime::future::Either;

use crate::utils::terminal::TerminalSize;

use super::LoggerTextItem;
use super::PromptTerminal;
use super::fits_terminal_height;
use super::terminal_too_small_text_items;

/// The state of a settings editor: some checkboxes followed by options that
/// each have a value, which may still be loading when the editor is shown.
pub struct SettingsEditorState {
  pub title: String,
  /// Whether to show the keys that do something for the active row dimmed
  /// under the title.
  pub show_key_help: bool,
  pub checkboxes_title: String,
  pub checkboxes: Vec<SettingsCheckbox>,
  pub options_title: String,
  pub options: SettingsOptions,
  /// Text typed by the user to narrow down the visible rows.
  filter: String,
  /// Index into the currently visible (filtered) rows.
  active_index: usize,
  /// First visible line, used to scroll long lists.
  scroll_offset: usize,
  /// The text of the value being typed for the active option.
  edit_text: Option<String>,
  /// Why the value being typed couldn't be accepted.
  edit_error: Option<String>,
  /// How many lines were shown at once the last time the editor was drawn,
  /// which is how far page up and page down go.
  page_size: usize,
}

pub struct SettingsCheckbox {
  pub text: String,
  pub is_checked: bool,
  /// Whether the text stands out (ex. a file type found in the current directory).
  pub is_highlighted: bool,
}

pub enum SettingsOptions {
  /// The options are on their way. The text says what's being waited on.
  Loading(String),
  /// There are no options to edit. The text says why.
  Unavailable(String),
  Loaded(Vec<SettingsOption>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SettingsOption {
  pub name: String,
  pub kind: SettingsOptionKind,
  /// The value that applies when the option isn't written, when it's known.
  pub default_value: Option<serde_json::Value>,
  /// The value to write. `None` leaves the option out so its default applies,
  /// and a value that's the same as the default is written anyway.
  pub value: Option<serde_json::Value>,
  pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SettingsOptionKind {
  Bool,
  /// One of these values.
  Enum(Vec<serde_json::Value>),
  Number,
  String,
  /// Shown, but too involved to edit here (ex. an array or object).
  ReadOnly,
}

impl SettingsEditorState {
  pub fn new(title: impl Into<String>) -> Self {
    SettingsEditorState {
      title: title.into(),
      show_key_help: false,
      checkboxes_title: String::new(),
      checkboxes: Vec::new(),
      options_title: String::new(),
      options: SettingsOptions::Loaded(Vec::new()),
      filter: String::new(),
      active_index: 0,
      scroll_offset: 0,
      edit_text: None,
      edit_error: None,
      page_size: 1,
    }
  }

  /// The loaded options, which is nothing while they're loading or unavailable.
  pub fn loaded_options(&self) -> &[SettingsOption] {
    match &self.options {
      SettingsOptions::Loaded(options) => options,
      SettingsOptions::Loading(_) | SettingsOptions::Unavailable(_) => &[],
    }
  }

  pub fn loaded_options_mut(&mut self) -> &mut [SettingsOption] {
    match &mut self.options {
      SettingsOptions::Loaded(options) => options,
      SettingsOptions::Loading(_) | SettingsOptions::Unavailable(_) => &mut [],
    }
  }

  /// Sets the result of loading the options.
  pub fn set_load_result(&mut self, result: Result<Vec<SettingsOption>>) {
    self.options = match result {
      Ok(options) => SettingsOptions::Loaded(options),
      Err(err) => SettingsOptions::Unavailable(format!("Could not load the options: {:#}", err)),
    };
  }
}

impl SettingsOption {
  /// The value that applies: the one to write, otherwise the default.
  pub fn effective_value(&self) -> Option<&serde_json::Value> {
    self.value.as_ref().or(self.default_value.as_ref())
  }

  /// Whether the option is written with the same value as its default.
  pub fn is_pinned(&self) -> bool {
    self.value.is_some() && self.value == self.default_value
  }
}

/// Shows the settings editor until the user leaves it. The checkboxes can be
/// used while `pending_options` resolves, and leaving before it does drops it
/// (ex. cancelling a download). The changes are read from the state.
pub async fn show_settings_editor(
  terminal: &impl PromptTerminal,
  state: &mut SettingsEditorState,
  pending_options: Option<LocalBoxFuture<'_, Result<Vec<SettingsOption>>>>,
) -> Result<()> {
  let result = run_settings_editor(terminal, state, pending_options).await;
  // what's shown next replaces the editor, which is nothing when it failed
  if result.is_err() {
    terminal.clear();
  }
  result
}

async fn run_settings_editor(
  terminal: &impl PromptTerminal,
  state: &mut SettingsEditorState,
  mut pending_options: Option<LocalBoxFuture<'_, Result<Vec<SettingsOption>>>>,
) -> Result<()> {
  // the options load while a key is waited on, and the same read is waited
  // on again once they have
  let mut pending_key = None;
  loop {
    let terminal_size = terminal.size();
    let mut max_visible_lines = max_visible_lines(state, terminal_size);
    let mut is_too_small = false;
    let text_items = loop {
      let text_items = render(state, max_visible_lines);
      // show less when lines wrapping would push the title off the screen
      if fits_terminal_height(&text_items, terminal_size) {
        break text_items;
      }
      if max_visible_lines == 1 {
        is_too_small = true;
        break terminal_too_small_text_items();
      }
      max_visible_lines -= 1;
    };
    state.page_size = max_visible_lines;
    terminal.draw(text_items);

    let mut key = pending_key.take().unwrap_or_else(|| terminal.read_key_async());
    let event = match pending_options.as_mut() {
      Some(options) => match future::select(&mut key, options).await {
        Either::Left((event, _)) => Some(event),
        Either::Right((result, _)) => {
          pending_options = None;
          pending_key = Some(key);
          state.set_load_result(result);
          None
        }
      },
      None => Some(key.await),
    };
    let Some(event) = event else {
      continue;
    };
    if let Event::Key(key_event) = event? {
      // what a key does can't be seen, so the only thing to do is leave
      if is_too_small && key_event.code != KeyCode::Esc {
        continue;
      }
      if handle_key(state, &key_event) == KeyOutcome::Close {
        return Ok(());
      }
    }
  }
}

#[derive(Debug, PartialEq, Eq)]
enum KeyOutcome {
  Continue,
  Close,
}

fn handle_key(state: &mut SettingsEditorState, key_event: &KeyEvent) -> KeyOutcome {
  if state.edit_text.is_some() {
    handle_edit_key(state, key_event);
    return KeyOutcome::Continue;
  }

  let rows = visible_rows(state);
  let active_row = rows.get(state.active_index).copied();
  match &key_event.code {
    KeyCode::Up => {
      if !rows.is_empty() {
        state.active_index = (state.active_index + rows.len() - 1) % rows.len();
      }
    }
    KeyCode::Down => {
      if !rows.is_empty() {
        state.active_index = (state.active_index + 1) % rows.len();
      }
    }
    KeyCode::PageUp => {
      state.active_index = state.active_index.saturating_sub(state.page_size.max(1));
    }
    KeyCode::PageDown => {
      state.active_index = state.active_index.saturating_add(state.page_size.max(1)).min(rows.len().saturating_sub(1));
    }
    KeyCode::Char(' ') | KeyCode::Enter => match active_row {
      Some(Row::Checkbox(index)) => {
        let checkbox = &mut state.checkboxes[index];
        checkbox.is_checked = !checkbox.is_checked;
      }
      Some(Row::Option(index)) => change_option(state, index, matches!(key_event.code, KeyCode::Enter)),
      None => {}
    },
    KeyCode::Tab => {
      if let Some(Row::Option(index)) = active_row {
        let option = &mut state.loaded_options_mut()[index];
        if option.kind == SettingsOptionKind::ReadOnly {
          // writing it would replace what's too involved to see here
        } else if option.value.is_none() {
          // write the default
          option.value = option.default_value.clone();
        }
      }
    }
    KeyCode::Delete => {
      if let Some(Row::Option(index)) = active_row {
        state.loaded_options_mut()[index].value = None;
      }
    }
    KeyCode::Backspace => {
      if state.filter.pop().is_some() {
        state.active_index = 0;
        state.scroll_offset = 0;
      } else if let Some(Row::Option(index)) = active_row {
        // it's the key that says delete on some keyboards
        state.loaded_options_mut()[index].value = None;
      }
    }
    KeyCode::Char(c) => {
      // any other printable character narrows down the rows
      if !key_event.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) {
        state.filter.push(*c);
        state.active_index = 0;
        state.scroll_offset = 0;
      }
    }
    KeyCode::Left | KeyCode::Esc => return KeyOutcome::Close,
    _ => {}
  }
  KeyOutcome::Continue
}

/// Changes the option the way its kind is changed: a boolean flips, an enum
/// moves to its next value, and a number or string starts being typed (only
/// on enter, because space is too easy to hit while moving through the list).
/// The value it's changed to is written even when it's the default, which is
/// what the user then asked for. Clearing the value is what leaves it out.
fn change_option(state: &mut SettingsEditorState, index: usize, is_enter: bool) {
  let option = &mut state.loaded_options_mut()[index];
  let kind = option.kind.clone();
  match &kind {
    SettingsOptionKind::Bool => {
      let current = option.effective_value().and_then(|value| value.as_bool()).unwrap_or(false);
      option.value = Some(serde_json::Value::Bool(!current));
    }
    SettingsOptionKind::Enum(values) => {
      if values.is_empty() {
        return;
      }
      let current_index = option.effective_value().and_then(|current| values.iter().position(|value| value == current));
      let next_index = current_index.map(|index| (index + 1) % values.len()).unwrap_or(0);
      let next_value = values[next_index].clone();
      option.value = Some(next_value);
    }
    SettingsOptionKind::Number | SettingsOptionKind::String => {
      if is_enter {
        let text = option.effective_value().map(value_text).unwrap_or_default();
        state.edit_text = Some(text);
        state.edit_error = None;
      }
    }
    SettingsOptionKind::ReadOnly => {}
  }
}

fn handle_edit_key(state: &mut SettingsEditorState, key_event: &KeyEvent) {
  match &key_event.code {
    KeyCode::Esc => {
      state.edit_text = None;
      state.edit_error = None;
    }
    KeyCode::Enter => commit_edit(state),
    KeyCode::Backspace => {
      if let Some(text) = &mut state.edit_text {
        text.pop();
        // the problem was with what the text used to be
        state.edit_error = None;
      }
    }
    KeyCode::Char(c) => {
      if !key_event.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        && let Some(text) = &mut state.edit_text
      {
        text.push(*c);
        state.edit_error = None;
      }
    }
    _ => {}
  }
}

fn commit_edit(state: &mut SettingsEditorState) {
  let rows = visible_rows(state);
  let (Some(text), Some(Row::Option(index))) = (state.edit_text.clone(), rows.get(state.active_index).copied()) else {
    state.edit_text = None;
    return;
  };
  // nothing was entered for an option that has nothing
  if text.is_empty() && state.loaded_options()[index].effective_value().is_none() {
    state.edit_text = None;
    state.edit_error = None;
    return;
  }
  let is_number = state.loaded_options()[index].kind == SettingsOptionKind::Number;
  let value = if is_number {
    match parse_number(&text) {
      Some(value) => value,
      None => {
        state.edit_error = Some("Expected a number.".to_string());
        return;
      }
    }
  } else {
    serde_json::Value::String(text)
  };
  state.loaded_options_mut()[index].value = Some(value);
  state.edit_text = None;
  state.edit_error = None;
}

fn parse_number(text: &str) -> Option<serde_json::Value> {
  let text = text.trim();
  if let Ok(value) = text.parse::<i64>() {
    return Some(value.into());
  }
  serde_json::Number::from_f64(text.parse::<f64>().ok()?).map(serde_json::Value::Number)
}

/// The keys that do something for the active row, so that nothing is offered
/// that wouldn't do anything where the user is.
fn key_help(state: &SettingsEditorState, active_row: Option<Row>) -> String {
  if state.edit_text.is_some() {
    return "enter: accept, esc: discard".to_string();
  }
  let mut keys = Vec::new();
  match active_row {
    Some(Row::Checkbox(_)) => keys.push("space: toggle"),
    Some(Row::Option(index)) => {
      let option = &state.loaded_options()[index];
      match &option.kind {
        SettingsOptionKind::Bool | SettingsOptionKind::Enum(_) => keys.push("space: change"),
        SettingsOptionKind::Number | SettingsOptionKind::String => keys.push("enter: edit"),
        SettingsOptionKind::ReadOnly => {}
      }
      if option.kind != SettingsOptionKind::ReadOnly && option.value.is_none() && option.default_value.is_some() {
        keys.push("tab: write default");
      }
      if option.value.is_some() {
        keys.push("del: clear");
      }
    }
    None => {}
  }
  keys.push("type to filter");
  keys.push("\u{2190}: back");
  keys.join(", ")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
  /// Index into the checkboxes.
  Checkbox(usize),
  /// Index into the loaded options.
  Option(usize),
}

/// The rows that match the current filter: the checkboxes then the options.
fn visible_rows(state: &SettingsEditorState) -> Vec<Row> {
  let filter = state.filter.to_lowercase();
  let matches = |text: &str| filter.is_empty() || text.to_lowercase().contains(&filter);
  let checkboxes = state
    .checkboxes
    .iter()
    .enumerate()
    .filter(|(_, checkbox)| matches(&checkbox.text))
    .map(|(i, _)| Row::Checkbox(i));
  let options = state
    .loaded_options()
    .iter()
    .enumerate()
    .filter(|(_, option)| matches(&option.name))
    .map(|(i, _)| Row::Option(i));
  checkboxes.chain(options).collect()
}

/// A line in the scrollable part of the editor.
struct Line {
  text: String,
  indent: u16,
  /// Whether this is the line of a row and not something about them (ex. a
  /// section's title).
  is_row: bool,
  /// Whether this is the line of the active row.
  is_active: bool,
}

/// The maximum number of scrollable lines to show at once, based on the
/// terminal height. When the terminal size is unknown everything is shown.
fn max_visible_lines(state: &SettingsEditorState, terminal_size: Option<TerminalSize>) -> usize {
  match terminal_size {
    Some(size) => {
      // reserve rows for: the title, the help line, the filter line, both scroll
      // indicators, the active option's description, and a little breathing room
      let reserved = 1 + usize::from(state.show_key_help) + usize::from(!state.filter.is_empty()) + 2 + 2 + 1;
      (size.rows as usize).saturating_sub(reserved).max(1)
    }
    None => usize::MAX,
  }
}

fn render(state: &mut SettingsEditorState, max_visible_lines: usize) -> Vec<LoggerTextItem> {
  let rows = visible_rows(state);
  // keep the active index within the visible rows
  if state.active_index >= rows.len() {
    state.active_index = rows.len().saturating_sub(1);
  }
  let lines = get_lines(state, &rows);
  update_scroll_offset(state, &lines, max_visible_lines);

  let mut result = vec![LoggerTextItem::Text(state.title.clone())];
  if state.show_key_help {
    result.push(LoggerTextItem::HangingText {
      text: format!("  {}", colors::gray(key_help(state, rows.get(state.active_index).copied()))),
      indent: 2,
    });
  }
  if !state.filter.is_empty() {
    result.push(LoggerTextItem::Text(format!("  filter: {}", state.filter)));
  }

  let end = state.scroll_offset.saturating_add(max_visible_lines).min(lines.len());
  push_hidden_lines(&mut result, &lines[..state.scroll_offset], "above");
  for line in &lines[state.scroll_offset..end] {
    result.push(line.as_text_item());
  }
  push_hidden_lines(&mut result, &lines[end..], "below");

  if let Some(error) = &state.edit_error {
    result.push(LoggerTextItem::HangingText {
      text: format!("  {}", colors::red(error)),
      indent: 2,
    });
  } else if let Some(Row::Option(index)) = rows.get(state.active_index)
    && let Some(description) = &state.loaded_options()[*index].description
  {
    result.push(LoggerTextItem::HangingText {
      text: format!("  {}", colors::gray(description)),
      indent: 2,
    });
  }
  result
}

/// Says how many rows are in the lines that are scrolled out of view. A line
/// on its own that isn't a row (ex. a section's title) is shown instead, since
/// it takes up as much room as saying that it's there.
fn push_hidden_lines(result: &mut Vec<LoggerTextItem>, hidden_lines: &[Line], direction: &str) {
  let row_count = hidden_lines.iter().filter(|line| line.is_row).count();
  match hidden_lines {
    [] => {}
    [line] if row_count == 0 => result.push(line.as_text_item()),
    _ if row_count == 0 => result.push(LoggerTextItem::Text(format!("  ...more {}", direction))),
    _ => result.push(LoggerTextItem::Text(format!("  ...{} more {}", row_count, direction))),
  }
}

/// Everything in the scrollable part of the editor: each section's title
/// followed by its rows or what's known about them.
fn get_lines(state: &SettingsEditorState, rows: &[Row]) -> Vec<Line> {
  let mut lines = Vec::new();
  let title_line = |title: &str| Line {
    text: format!("  {}", title),
    indent: 2,
    is_row: false,
    is_active: false,
  };
  let note_line = |text: &str| Line {
    text: format!("    {}", colors::gray(text)),
    indent: 4,
    is_row: false,
    is_active: false,
  };

  let has_checkbox_rows = rows.iter().any(|row| matches!(row, Row::Checkbox(_)));
  if has_checkbox_rows {
    lines.push(title_line(&state.checkboxes_title));
  }
  let mut has_added_options_title = false;
  for (row_index, row) in rows.iter().enumerate() {
    let is_active = row_index == state.active_index;
    let prefix = if is_active { ">" } else { " " };
    match row {
      Row::Checkbox(index) => {
        let checkbox = &state.checkboxes[*index];
        let text = if checkbox.is_highlighted {
          colors::cyan(&checkbox.text).to_string()
        } else {
          checkbox.text.clone()
        };
        lines.push(Line {
          text: format!("{} [{}] {}", prefix, if checkbox.is_checked { "x" } else { " " }, text),
          indent: 6,
          is_row: true,
          is_active,
        });
      }
      Row::Option(index) => {
        if !has_added_options_title {
          lines.push(title_line(&state.options_title));
          has_added_options_title = true;
        }
        let option = &state.loaded_options()[*index];
        let edit_text = if is_active { state.edit_text.as_deref() } else { None };
        lines.push(Line {
          text: format!("{}   {}", prefix, render_option(option, edit_text)),
          indent: 6,
          is_row: true,
          is_active,
        });
      }
    }
  }

  match &state.options {
    SettingsOptions::Loading(text) | SettingsOptions::Unavailable(text) => {
      lines.push(title_line(&state.options_title));
      lines.push(note_line(text));
    }
    SettingsOptions::Loaded(options) => {
      if !has_added_options_title && !options.is_empty() {
        lines.push(title_line(&state.options_title));
        lines.push(note_line("(no matching options)"));
      }
    }
  }
  lines
}

/// An option as it's shown: dimmed when it's left at its default, since that
/// isn't written, and with its value standing out when it was changed.
fn render_option(option: &SettingsOption, edit_text: Option<&str>) -> String {
  if let Some(edit_text) = edit_text {
    // show a cursor (block character)
    return format!("{}: {}\u{2588}", option.name, edit_text);
  }
  let read_only_note = if option.kind == SettingsOptionKind::ReadOnly {
    " (edit in the config file)"
  } else {
    ""
  };
  match (&option.value, &option.default_value) {
    (Some(value), _) if option.is_pinned() => {
      format!("{}: {}{}", option.name, value_text(value), colors::gray(" (default, written)"))
    }
    (Some(value), _) if read_only_note.is_empty() => format!("{}: {}", option.name, colors::cyan(value_text(value))),
    (Some(value), _) => format!("{}: {}{}", option.name, colors::cyan(value_text(value)), colors::gray(read_only_note)),
    (None, Some(default_value)) => colors::gray(format!("{}: {}{}", option.name, value_text(default_value), read_only_note)).to_string(),
    (None, None) => colors::gray(format!("{}: (not set){}", option.name, read_only_note)).to_string(),
  }
}

/// A value as it's shown and typed: a string without its quotes.
fn value_text(value: &serde_json::Value) -> String {
  match value {
    serde_json::Value::String(text) => text.clone(),
    value => value.to_string(),
  }
}

impl Line {
  fn as_text_item(&self) -> LoggerTextItem {
    LoggerTextItem::HangingText {
      text: self.text.clone(),
      indent: self.indent,
    }
  }
}

/// Adjusts the scroll offset so the active row stays within the visible
/// window, along with the title above the first row when there's room for both.
fn update_scroll_offset(state: &mut SettingsEditorState, lines: &[Line], max_visible_lines: usize) {
  if let Some(active_line) = lines.iter().position(|line| line.is_active) {
    let first_wanted_line = if active_line == 1 && max_visible_lines > 1 { 0 } else { active_line };
    if first_wanted_line < state.scroll_offset {
      state.scroll_offset = first_wanted_line;
    } else if active_line >= state.scroll_offset.saturating_add(max_visible_lines) {
      state.scroll_offset = active_line + 1 - max_visible_lines;
    }
  }
  let max_scroll = lines.len().saturating_sub(max_visible_lines);
  if state.scroll_offset > max_scroll {
    state.scroll_offset = max_scroll;
  }
}

#[cfg(test)]
mod test {
  use super::*;
  use pretty_assertions::assert_eq;
  use serde_json::json;

  fn option(name: &str, kind: SettingsOptionKind, default_value: serde_json::Value) -> SettingsOption {
    SettingsOption {
      name: name.to_string(),
      kind,
      default_value: Some(default_value),
      value: None,
      description: None,
    }
  }

  fn build_state() -> SettingsEditorState {
    let mut state = SettingsEditorState::new("plugin");
    state.checkboxes_title = "File types:".to_string();
    state.checkboxes = vec![
      SettingsCheckbox {
        text: ".ts".to_string(),
        is_checked: true,
        is_highlighted: false,
      },
      SettingsCheckbox {
        text: ".js".to_string(),
        is_checked: true,
        is_highlighted: false,
      },
    ];
    state.options_title = "Options:".to_string();
    state.options = SettingsOptions::Loaded(vec![
      option("semi", SettingsOptionKind::Bool, json!(true)),
      option("quotes", SettingsOptionKind::Enum(vec![json!("double"), json!("single")]), json!("double")),
      option("width", SettingsOptionKind::Number, json!(120)),
      option("commands", SettingsOptionKind::ReadOnly, json!([])),
    ]);
    state
  }

  fn press(state: &mut SettingsEditorState, code: KeyCode) -> KeyOutcome {
    handle_key(state, &KeyEvent::new(code, KeyModifiers::NONE))
  }

  fn type_text(state: &mut SettingsEditorState, text: &str) {
    for c in text.chars() {
      press(state, KeyCode::Char(c));
    }
  }

  fn rendered_lines(state: &mut SettingsEditorState) -> Vec<String> {
    render(state, usize::MAX)
      .into_iter()
      .map(|item| match item {
        LoggerTextItem::Text(text) => text,
        LoggerTextItem::HangingText { text, .. } => text,
      })
      .collect()
  }

  fn option_value(state: &SettingsEditorState, name: &str) -> Option<serde_json::Value> {
    state.loaded_options().iter().find(|option| option.name == name).unwrap().value.clone()
  }

  #[test]
  fn renders_checkboxes_and_options() {
    let mut state = build_state();
    state.show_key_help = true;
    state.loaded_options_mut()[1].value = Some(json!("single"));
    state.loaded_options_mut()[2].value = Some(json!(120));
    assert_eq!(
      rendered_lines(&mut state),
      vec![
        "plugin".to_string(),
        format!("  {}", colors::gray("space: toggle, type to filter, \u{2190}: back")),
        "  File types:".to_string(),
        "> [x] .ts".to_string(),
        "  [x] .js".to_string(),
        "  Options:".to_string(),
        format!("    {}", colors::gray("semi: true")),
        format!("    quotes: {}", colors::cyan("single")),
        format!("    width: 120{}", colors::gray(" (default, written)")),
        format!("    {}", colors::gray("commands: [] (edit in the config file)")),
      ]
    );
  }

  #[test]
  fn renders_what_is_known_about_options_that_are_not_loaded() {
    let mut state = build_state();
    state.options = SettingsOptions::Loading("Downloading... (esc to cancel)".to_string());
    assert_eq!(
      rendered_lines(&mut state)[4..],
      ["  Options:".to_string(), format!("    {}", colors::gray("Downloading... (esc to cancel)"))]
    );
    state.set_load_result(Err(anyhow::anyhow!("failed")));
    assert_eq!(
      rendered_lines(&mut state)[5],
      format!("    {}", colors::gray("Could not load the options: failed"))
    );
  }

  #[test]
  fn toggles_checkboxes_while_the_options_load() {
    let mut state = build_state();
    state.options = SettingsOptions::Loading("Loading".to_string());
    press(&mut state, KeyCode::Down);
    press(&mut state, KeyCode::Char(' '));
    assert!(state.checkboxes[0].is_checked);
    assert!(!state.checkboxes[1].is_checked);
    // there are no option rows to move to yet
    press(&mut state, KeyCode::Down);
    assert_eq!(state.active_index, 0);
  }

  #[test]
  fn changing_a_value_to_its_default_writes_it_until_it_is_cleared() {
    let mut state = build_state();
    state.active_index = 2; // semi
    press(&mut state, KeyCode::Char(' '));
    assert_eq!(option_value(&state, "semi"), Some(json!(false)));
    press(&mut state, KeyCode::Char(' '));
    assert_eq!(option_value(&state, "semi"), Some(json!(true)));
    assert_eq!(rendered_lines(&mut state)[5], format!(">   semi: true{}", colors::gray(" (default, written)")));
    press(&mut state, KeyCode::Delete);
    assert_eq!(option_value(&state, "semi"), None);

    state.active_index = 3; // quotes
    press(&mut state, KeyCode::Enter);
    assert_eq!(option_value(&state, "quotes"), Some(json!("single")));
    press(&mut state, KeyCode::Enter);
    assert_eq!(option_value(&state, "quotes"), Some(json!("double")));
    // the key that says delete on some keyboards
    press(&mut state, KeyCode::Backspace);
    assert_eq!(option_value(&state, "quotes"), None);

    // accepting what's there to be typed over is asking for it too
    state.active_index = 4; // width
    press(&mut state, KeyCode::Enter);
    press(&mut state, KeyCode::Enter);
    assert_eq!(option_value(&state, "width"), Some(json!(120)));

    // backspace is for the filter while there is one
    type_text(&mut state, "wid");
    for _ in 0..3 {
      assert_eq!(option_value(&state, "width"), Some(json!(120)));
      press(&mut state, KeyCode::Backspace);
    }
    state.active_index = 4;
    press(&mut state, KeyCode::Backspace);
    assert_eq!(option_value(&state, "width"), None);
  }

  #[test]
  fn tab_writes_the_default() {
    let mut state = build_state();
    state.active_index = 2; // semi
    press(&mut state, KeyCode::Tab);
    assert_eq!(option_value(&state, "semi"), Some(json!(true)));
    assert!(state.loaded_options()[0].is_pinned());
    // which is left alone from then on, the same as a changed value
    press(&mut state, KeyCode::Tab);
    assert_eq!(option_value(&state, "semi"), Some(json!(true)));
    press(&mut state, KeyCode::Char(' '));
    press(&mut state, KeyCode::Tab);
    assert_eq!(option_value(&state, "semi"), Some(json!(false)));
  }

  #[test]
  fn tab_does_not_write_an_option_that_can_not_be_edited() {
    let mut state = build_state();
    state.active_index = 5; // commands
    press(&mut state, KeyCode::Tab);
    assert_eq!(option_value(&state, "commands"), None);
  }

  #[test]
  fn key_help_only_has_the_keys_that_do_something_for_the_active_row() {
    let mut state = build_state();
    let key_help = |state: &SettingsEditorState| key_help(state, visible_rows(state).get(state.active_index).copied());
    assert_eq!(key_help(&state), "space: toggle, type to filter, \u{2190}: back");
    state.active_index = 2; // semi
    assert_eq!(key_help(&state), "space: change, tab: write default, type to filter, \u{2190}: back");
    press(&mut state, KeyCode::Tab);
    assert_eq!(key_help(&state), "space: change, del: clear, type to filter, \u{2190}: back");
    press(&mut state, KeyCode::Char(' '));
    assert_eq!(key_help(&state), "space: change, del: clear, type to filter, \u{2190}: back");
    state.active_index = 4; // width
    assert_eq!(key_help(&state), "enter: edit, tab: write default, type to filter, \u{2190}: back");
    press(&mut state, KeyCode::Enter);
    assert_eq!(key_help(&state), "enter: accept, esc: discard");
    press(&mut state, KeyCode::Esc);
    state.active_index = 5; // commands
    assert_eq!(key_help(&state), "type to filter, \u{2190}: back");
  }

  #[test]
  fn page_keys_move_by_what_was_shown_without_wrapping() {
    let mut state = build_state();
    state.page_size = 4;
    press(&mut state, KeyCode::PageDown);
    assert_eq!(state.active_index, 4);
    press(&mut state, KeyCode::PageDown);
    assert_eq!(state.active_index, 5);
    press(&mut state, KeyCode::PageUp);
    assert_eq!(state.active_index, 1);
    press(&mut state, KeyCode::PageUp);
    assert_eq!(state.active_index, 0);
  }

  #[test]
  fn types_a_number_after_pressing_enter() {
    let mut state = build_state();
    state.active_index = 4; // width
    // space doesn't start typing
    press(&mut state, KeyCode::Char(' '));
    assert_eq!(state.edit_text, None);
    press(&mut state, KeyCode::Enter);
    assert_eq!(state.edit_text.as_deref(), Some("120"));
    assert_eq!(rendered_lines(&mut state)[7], ">   width: 120\u{2588}");
    for _ in 0..3 {
      press(&mut state, KeyCode::Backspace);
    }
    type_text(&mut state, "abc");
    press(&mut state, KeyCode::Enter);
    // still typing because it's not a number
    assert_eq!(state.edit_error.as_deref(), Some("Expected a number."));
    assert_eq!(rendered_lines(&mut state).last().unwrap(), &format!("  {}", colors::red("Expected a number.")));
    for _ in 0..3 {
      press(&mut state, KeyCode::Backspace);
    }
    type_text(&mut state, "80");
    press(&mut state, KeyCode::Enter);
    assert_eq!(state.edit_text, None);
    assert_eq!(option_value(&state, "width"), Some(json!(80)));

    // escape stops typing without leaving the editor
    press(&mut state, KeyCode::Enter);
    type_text(&mut state, "1");
    assert_eq!(press(&mut state, KeyCode::Esc), KeyOutcome::Continue);
    assert_eq!(option_value(&state, "width"), Some(json!(80)));
    assert_eq!(press(&mut state, KeyCode::Esc), KeyOutcome::Close);
  }

  #[test]
  fn accepting_nothing_leaves_an_option_without_a_value_as_it_was() {
    let mut state = build_state();
    let mut option = option("ignoreComment", SettingsOptionKind::String, json!(""));
    option.default_value = None;
    state.options = SettingsOptions::Loaded(vec![option]);
    state.active_index = 2;
    press(&mut state, KeyCode::Enter);
    assert_eq!(state.edit_text.as_deref(), Some(""));
    press(&mut state, KeyCode::Enter);
    assert_eq!(state.edit_text, None);
    assert_eq!(option_value(&state, "ignoreComment"), None);

    press(&mut state, KeyCode::Enter);
    type_text(&mut state, "ignore");
    press(&mut state, KeyCode::Enter);
    assert_eq!(option_value(&state, "ignoreComment"), Some(json!("ignore")));
  }

  #[test]
  fn read_only_options_are_not_changed() {
    let mut state = build_state();
    state.active_index = 5; // commands
    press(&mut state, KeyCode::Enter);
    press(&mut state, KeyCode::Char(' '));
    assert_eq!(state.edit_text, None);
    assert_eq!(option_value(&state, "commands"), None);
  }

  #[test]
  fn typing_filters_the_rows() {
    let mut state = build_state();
    type_text(&mut state, "qu");
    assert_eq!(visible_rows(&state), vec![Row::Option(1)]);
    assert_eq!(
      rendered_lines(&mut state),
      vec![
        "plugin".to_string(),
        "  filter: qu".to_string(),
        "  Options:".to_string(),
        format!(">   {}", colors::gray("quotes: double")),
      ]
    );
    press(&mut state, KeyCode::Enter);
    assert_eq!(option_value(&state, "quotes"), Some(json!("single")));
    type_text(&mut state, "x");
    assert_eq!(
      rendered_lines(&mut state)[2..],
      ["  Options:".to_string(), format!("    {}", colors::gray("(no matching options)"))]
    );
  }

  #[test]
  fn left_and_escape_leave_the_editor() {
    let mut state = build_state();
    assert_eq!(press(&mut state, KeyCode::Left), KeyOutcome::Close);
    assert_eq!(press(&mut state, KeyCode::Esc), KeyOutcome::Close);
  }

  #[test]
  fn scrolls_to_keep_the_active_row_and_its_title_visible() {
    let mut state = build_state();
    state.active_index = 5;
    let lines = render(&mut state, 3);
    assert_eq!(state.scroll_offset, 5);
    assert_eq!(lines.len(), 5); // title, more above, 3 lines
    // going back to the first row shows its section's title
    state.active_index = 0;
    render(&mut state, 3);
    assert_eq!(state.scroll_offset, 0);
  }
}
