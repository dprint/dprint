use anyhow::Result;
use crossterm::event::read;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use crossterm::terminal;

#[derive(Debug, Copy, Clone)]
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

pub(crate) fn read_terminal_key_press() -> Result<Event> {
  fn read_until_key_press() -> Result<Event> {
    loop {
      let result = read();
      if let Ok(Event::Key(e)) = &result {
        if e.kind == KeyEventKind::Press {
          return Ok(result?);
        }
      }
    }
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
