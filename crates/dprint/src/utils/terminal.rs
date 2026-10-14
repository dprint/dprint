use std::io;
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
  // https://github.com/crossterm-rs/crossterm/issues/521
  let was_raw_mode_enabled = terminal::is_raw_mode_enabled()?;
  if !was_raw_mode_enabled {
    terminal::enable_raw_mode()?;
  }

  let result = read_key_press_or_resize(&mut CrosstermEvents);
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

/// Where the terminal's events are read from, which tests provide their own of.
trait TerminalEvents {
  /// Waits for the next event.
  fn read(&mut self) -> io::Result<Event>;
  /// Whether there's an event to read before the time is up.
  fn poll(&mut self, timeout: Duration) -> io::Result<bool>;
}

struct CrosstermEvents;

impl TerminalEvents for CrosstermEvents {
  fn read(&mut self) -> io::Result<Event> {
    read()
  }

  fn poll(&mut self, timeout: Duration) -> io::Result<bool> {
    poll(timeout)
  }
}

fn read_key_press_or_resize(events: &mut impl TerminalEvents) -> Result<Event> {
  loop {
    match events.read()? {
      Event::Key(e) if e.kind == KeyEventKind::Press => return Ok(Event::Key(e)),
      event @ Event::Resize(..) => return read_until_resize_settles(events, event),
      _ => {}
    }
  }
}

fn read_until_resize_settles(events: &mut impl TerminalEvents, mut resize_event: Event) -> Result<Event> {
  while events.poll(RESIZE_SETTLE_TIME)? {
    match events.read()? {
      // handling the key draws for the new size anyway
      Event::Key(e) if e.kind == KeyEventKind::Press => return Ok(Event::Key(e)),
      event @ Event::Resize(..) => resize_event = event,
      _ => {}
    }
  }
  Ok(resize_event)
}

#[cfg(test)]
mod test {
  use std::collections::VecDeque;

  use pretty_assertions::assert_eq;

  use super::*;

  /// Events that each arrive some time after the one before them.
  struct TestEvents {
    events: VecDeque<(Duration, io::Result<Event>)>,
    /// How long was waited on events in total.
    waited: Duration,
  }

  impl TestEvents {
    fn new(events: Vec<(u64, io::Result<Event>)>) -> Self {
      TestEvents {
        events: events.into_iter().map(|(delay, event)| (Duration::from_millis(delay), event)).collect(),
        waited: Duration::ZERO,
      }
    }
  }

  impl TerminalEvents for TestEvents {
    fn read(&mut self) -> io::Result<Event> {
      let (delay, event) = self.events.pop_front().expect("nothing was left to read");
      self.waited += delay;
      event
    }

    fn poll(&mut self, timeout: Duration) -> io::Result<bool> {
      match self.events.front_mut() {
        Some((delay, _)) if *delay <= timeout => Ok(true),
        Some((delay, _)) => {
          // it arrives that much sooner the next time it's waited on
          *delay -= timeout;
          self.waited += timeout;
          Ok(false)
        }
        None => {
          self.waited += timeout;
          Ok(false)
        }
      }
    }
  }

  fn key(code: KeyCode, kind: KeyEventKind) -> io::Result<Event> {
    Ok(Event::Key(KeyEvent::new_with_kind(code, KeyModifiers::NONE, kind)))
  }

  fn press(code: KeyCode) -> io::Result<Event> {
    key(code, KeyEventKind::Press)
  }

  fn resize(cols: u16, rows: u16) -> io::Result<Event> {
    Ok(Event::Resize(cols, rows))
  }

  #[test]
  fn reads_the_next_key_press() {
    let mut events = TestEvents::new(vec![
      // only a key going down counts, which is all some terminals report
      (0, key(KeyCode::Char('a'), KeyEventKind::Release)),
      (0, Ok(Event::FocusGained)),
      (0, press(KeyCode::Char('b'))),
      (0, press(KeyCode::Char('c'))),
    ]);
    assert_eq!(read_key_press_or_resize(&mut events).unwrap(), press(KeyCode::Char('b')).unwrap());
    assert_eq!(events.events.len(), 1);
    assert_eq!(events.waited, Duration::ZERO);
  }

  #[test]
  fn reports_a_resize_once_the_size_stops_changing() {
    let settle_time = RESIZE_SETTLE_TIME.as_millis() as u64;
    let mut events = TestEvents::new(vec![
      (0, resize(80, 24)),
      (10, resize(79, 24)),
      // just in time to still be the same resize
      (settle_time, resize(78, 24)),
      // not something that says the size is still changing
      (10, key(KeyCode::Char('a'), KeyEventKind::Release)),
      // too late, so it's a resize of its own
      (settle_time + 1, resize(60, 24)),
      (500, press(KeyCode::Enter)),
    ]);
    assert_eq!(read_key_press_or_resize(&mut events).unwrap(), Event::Resize(78, 24));
    // it waited for the size to settle after the last of them
    assert_eq!(events.waited, Duration::from_millis(10 + settle_time + 10 + settle_time));
    assert_eq!(events.events.len(), 2);

    // which isn't held up by the key that's pressed long after it
    assert_eq!(read_key_press_or_resize(&mut events).unwrap(), Event::Resize(60, 24));
    assert_eq!(events.events.len(), 1);
    assert_eq!(read_key_press_or_resize(&mut events).unwrap(), press(KeyCode::Enter).unwrap());
  }

  #[test]
  fn reads_a_key_pressed_while_the_size_is_changing_right_away() {
    let mut events = TestEvents::new(vec![(0, resize(80, 24)), (10, press(KeyCode::Down)), (0, resize(70, 24))]);
    assert_eq!(read_key_press_or_resize(&mut events).unwrap(), press(KeyCode::Down).unwrap());
    assert_eq!(events.waited, Duration::from_millis(10));
    assert_eq!(events.events.len(), 1);
  }

  #[test]
  fn fails_when_the_terminal_can_not_be_read() {
    let mut events = TestEvents::new(vec![(0, Err(io::Error::other("closed")))]);
    assert_eq!(read_key_press_or_resize(&mut events).unwrap_err().to_string(), "closed");
    let mut events = TestEvents::new(vec![(0, resize(80, 24)), (0, Err(io::Error::other("closed")))]);
    assert_eq!(read_key_press_or_resize(&mut events).unwrap_err().to_string(), "closed");
  }
}
