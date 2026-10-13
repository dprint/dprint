use std::io::IsTerminal;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use crossterm::event::poll;
use crossterm::event::read;
use crossterm::terminal;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct TerminalSize {
  pub cols: u16,
  pub rows: u16,
}

/// Gets the terminal size.
pub fn get_terminal_size() -> Option<TerminalSize> {
  match crossterm::terminal::size() {
    Ok(size) => Some(TerminalSize { cols: size.0, rows: size.1 }),
    Err(_) => None,
  }
}

/// If the terminal is interactive.
pub fn is_terminal_interactive() -> bool {
  std::io::stderr().is_terminal() && get_terminal_size().is_some()
}

/// How long the terminal's size has to stay the same for a resize to be
/// reported, so dragging its edge is a single resize instead of many.
const RESIZE_SETTLE_TIME: Duration = Duration::from_millis(75);

/// Waits for a key to be pressed. This also returns once the terminal was
/// resized, so what's shown can be drawn again for its new size.
pub(crate) fn read_terminal_key_press() -> Result<Event> {
  fn read_until_key_press() -> Result<Event> {
    loop {
      match read()? {
        Event::Key(e) if e.kind == KeyEventKind::Press => return Ok(Event::Key(e)),
        event @ Event::Resize(..) => return read_until_resize_settles(event),
        _ => {}
      }
    }
  }

  fn read_until_resize_settles(mut resize_event: Event) -> Result<Event> {
    while poll(RESIZE_SETTLE_TIME)? {
      match read()? {
        // handling the key draws for the new size anyway
        Event::Key(e) if e.kind == KeyEventKind::Press => return Ok(Event::Key(e)),
        event @ Event::Resize(..) => resize_event = event,
        _ => {}
      }
    }
    Ok(resize_event)
  }

  // https://github.com/crossterm-rs/crossterm/issues/521
  let was_raw_mode_enabled = terminal::is_raw_mode_enabled()?;
  if !was_raw_mode_enabled {
    terminal::enable_raw_mode()?;
  }

  let result = read_until_key_press();
  if !was_raw_mode_enabled {
    terminal::disable_raw_mode()?;
  }
  match result {
    Ok(Event::Key(KeyEvent {
      code: KeyCode::Char('c'),
      modifiers: KeyModifiers::CONTROL,
      kind,
      state,
    })) => Ok(Event::Key(KeyEvent {
      code: KeyCode::Esc,
      modifiers: KeyModifiers::NONE,
      kind,
      state,
    })),
    _ => Ok(result?),
  }
}
