use crate::types::ErrBox;
use crossterm::event::read;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use crossterm::terminal;

pub fn get_terminal_width() -> Option<u16> {
  get_terminal_size().map(|(cols, _)| cols)
}

/// Gets the terminal size (width/cols, height/rows).
pub fn get_terminal_size() -> Option<(u16, u16)> {
  match crossterm::terminal::size() {
    Ok(size) => Some(size),
    Err(_) => None,
  }
}

pub(crate) fn read_terminal_event() -> Result<Event, ErrBox> {
  // https://github.com/crossterm-rs/crossterm/issues/521
  terminal::enable_raw_mode()?;
  let result = read();
  terminal::disable_raw_mode()?;
  match result {
    Ok(Event::Key(KeyEvent {
      code: KeyCode::Char('c'),
      modifiers: KeyModifiers::CONTROL,
    })) => Ok(Event::Key(KeyEvent {
      code: KeyCode::Esc,
      modifiers: KeyModifiers::NONE,
    })),
    _ => Ok(result?),
  }
}
