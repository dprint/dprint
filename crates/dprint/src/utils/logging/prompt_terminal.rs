use anyhow::Result;
use anyhow::anyhow;
use console_static_text::ConsoleSize;
use console_static_text::ConsoleStaticText;
use crossterm::event::Event;
use dprint_core::async_runtime::FutureExt;
use dprint_core::async_runtime::LocalBoxFuture;

use crate::utils::terminal::TerminalSize;
use crate::utils::terminal::get_terminal_size;
use crate::utils::terminal::read_terminal_key_press;

use super::Logger;
use super::LoggerRefreshItemKind;
use super::LoggerTextItem;

/// What an interactive prompt draws itself on and reads its keys from. The
/// prompts only use the terminal through this, so everything they do can be
/// driven with scripted keys in tests.
pub trait PromptTerminal {
  /// The size of the terminal, which isn't known when it's not a tty.
  fn size(&self) -> Option<TerminalSize>;
  /// Draws the prompt in place of what it last drew.
  fn draw(&self, text_items: Vec<LoggerTextItem>);
  /// Removes what the prompt last drew.
  fn clear(&self);
  /// Logs text that stays in the terminal once the prompt is gone.
  fn log(&self, text_items: &[LoggerTextItem], context_name: &str);
  /// Waits for a key to be pressed without blocking the thread, so something
  /// else can happen in the meantime (ex. a download). The event is
  /// something else when the prompt only has to draw itself again (ex. the
  /// terminal was resized). The read is not cancelled by dropping the
  /// future, so wait on the same future again instead of asking for another
  /// key.
  fn read_key(&self) -> LocalBoxFuture<'static, Result<Event>>;
}

/// Whether the text items can be drawn in a terminal of the size without
/// their first lines being cut off, which depends on how they wrap.
pub fn fits_terminal_height(text_items: &[LoggerTextItem], terminal_size: Option<TerminalSize>) -> bool {
  let Some(terminal_size) = terminal_size else {
    return true;
  };
  let size = ConsoleSize {
    cols: Some(terminal_size.cols),
    rows: None,
  };
  let text_items = text_items.iter().map(|item| item.as_static_text_item()).collect::<Vec<_>>();
  let text = ConsoleStaticText::new(move || size)
    .render_items_with_size(text_items.iter(), size)
    .unwrap_or_default();
  // leave a row for the cursor
  text
    .split(
      "
",
    )
    .count()
    < terminal_size.rows as usize
}

/// What a prompt shows in place of itself when it can't be made to fit in
/// the terminal, which is drawn again once the terminal is resized.
pub fn terminal_too_small_text_items() -> Vec<LoggerTextItem> {
  vec![LoggerTextItem::Text(
    "The terminal is too small to show this. Make it bigger to continue.".to_string(),
  )]
}

/// The terminal of the process, drawn to through the logger.
pub struct LoggerPromptTerminal<'a> {
  logger: &'a Logger,
}

impl<'a> LoggerPromptTerminal<'a> {
  pub fn new(logger: &'a Logger) -> Self {
    LoggerPromptTerminal { logger }
  }
}

impl PromptTerminal for LoggerPromptTerminal<'_> {
  fn size(&self) -> Option<TerminalSize> {
    get_terminal_size()
  }

  fn draw(&self, text_items: Vec<LoggerTextItem>) {
    self.logger.set_refresh_item(LoggerRefreshItemKind::Selection, text_items);
  }

  fn clear(&self) {
    self.logger.remove_refresh_item(LoggerRefreshItemKind::Selection);
  }

  fn log(&self, text_items: &[LoggerTextItem], context_name: &str) {
    self.logger.log_text_items(text_items, context_name);
  }

  fn read_key(&self) -> LocalBoxFuture<'static, Result<Event>> {
    let read = dprint_core::async_runtime::spawn_blocking(read_terminal_key_press);
    async move { read.await.map_err(|err| anyhow!("Failed reading key press: {:#}", err))? }.boxed_local()
  }
}

#[cfg(test)]
pub use scripted::*;

#[cfg(test)]
mod scripted {
  use std::collections::VecDeque;
  use std::sync::Arc;
  use std::task::Poll;
  use std::time::Duration;
  use std::time::Instant;

  use anyhow::bail;
  use console_static_text::ansi::strip_ansi_codes;
  use crossterm::event::KeyCode;
  use crossterm::event::KeyEvent;
  use crossterm::event::KeyModifiers;
  use parking_lot::Mutex;

  use super::*;

  /// How long a script waits on something that's loading before it's
  /// considered to never finish.
  const WAIT_TIMEOUT: Duration = Duration::from_secs(60);

  /// A terminal for tests that presses the keys of a script and keeps what
  /// was on the screen after each of its steps.
  #[derive(Clone)]
  pub struct ScriptedPromptTerminal(Arc<Mutex<ScriptedState>>);

  /// What was on the screen once a step of a script was done.
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct PromptScreen {
    /// The text of the step in the script, or `start` for what was shown
    /// before the first key was pressed.
    pub step: String,
    /// The lines the prompt left on the screen, wrapped and cut off the way
    /// the terminal would. Nothing when it removed itself.
    pub lines: Vec<String>,
  }

  struct ScriptedState {
    size: Option<TerminalSize>,
    steps: VecDeque<ScriptStep>,
    screens: Vec<PromptScreen>,
    /// The step the screen is currently being drawn for.
    current: Option<PromptScreen>,
    draw_count: usize,
    /// Whether a key was asked for after the last step.
    is_out_of_steps: bool,
  }

  struct ScriptStep {
    text: String,
    kind: ScriptStepKind,
    /// Whether the screen is being drawn for this step.
    is_started: bool,
  }

  enum ScriptStepKind {
    /// The keys that are left to press.
    Keys(VecDeque<KeyEvent>),
    /// Don't press anything until the prompt draws itself again by its own
    /// doing (ex. the options it was loading are there).
    Wait,
    /// Resize the terminal.
    Resize(TerminalSize),
  }

  impl ScriptedPromptTerminal {
    /// Creates a terminal that presses what the steps say. A step is:
    ///
    /// - a key (`up`, `down`, `left`, `right`, `enter`, `esc`, `tab`,
    ///   `space`, `backspace`, `delete`, `pageup`, `pagedown`), optionally with modifiers
    ///   (ex. `shift+down`, `ctrl+c`),
    /// - text to type in quotes (ex. `"type"`),
    /// - `resize <cols>x<rows>`, which resizes the terminal, or
    /// - `wait`, which waits for what's loading.
    pub fn new(steps: &[&str], size: Option<TerminalSize>) -> Result<Self> {
      let steps = steps.iter().map(|step| parse_step(step)).collect::<Result<VecDeque<_>>>()?;
      Ok(ScriptedPromptTerminal(Arc::new(Mutex::new(ScriptedState {
        size,
        steps,
        screens: Vec::new(),
        current: Some(PromptScreen {
          step: "start".to_string(),
          lines: Vec::new(),
        }),
        draw_count: 0,
        is_out_of_steps: false,
      }))))
    }

    /// What was on the screen after each step that has been done so far.
    pub fn take_screens(&self) -> Vec<PromptScreen> {
      let mut state = self.0.lock();
      let mut screens = std::mem::take(&mut state.screens);
      screens.extend(state.current.take());
      screens
    }

    /// Whether a prompt wanted another key once every step was done.
    pub fn is_out_of_steps(&self) -> bool {
      self.0.lock().is_out_of_steps
    }

    /// The steps that weren't done because nothing asked for a key anymore.
    pub fn remaining_steps(&self) -> Vec<String> {
      self.0.lock().steps.iter().map(|step| step.text.clone()).collect()
    }

    fn set_lines(&self, text_items: &[LoggerTextItem]) {
      let mut state = self.0.lock();
      let lines = render_lines(text_items, state.size);
      state.draw_count += 1;
      if let Some(current) = state.current.as_mut() {
        current.lines = lines;
      }
    }
  }

  impl PromptTerminal for ScriptedPromptTerminal {
    fn size(&self) -> Option<TerminalSize> {
      self.0.lock().size
    }

    fn draw(&self, text_items: Vec<LoggerTextItem>) {
      self.set_lines(&text_items);
    }

    fn clear(&self) {
      self.set_lines(&[]);
    }

    fn log(&self, text_items: &[LoggerTextItem], _context_name: &str) {
      self.set_lines(text_items);
    }

    fn read_key(&self) -> LocalBoxFuture<'static, Result<Event>> {
      let terminal = self.clone();
      let mut wait_start = None;
      std::future::poll_fn(move |cx| {
        let mut state = terminal.0.lock();
        let (draw_count, started) = *wait_start.get_or_insert_with(|| (state.draw_count, Instant::now()));
        match state.next_event(draw_count) {
          Ok(Some(event)) => Poll::Ready(Ok(event)),
          Ok(None) if started.elapsed() > WAIT_TIMEOUT => Poll::Ready(Err(anyhow!("The script waited, but nothing finished loading."))),
          Ok(None) => {
            // nothing says when the screen gets drawn, so keep checking
            cx.waker().wake_by_ref();
            Poll::Pending
          }
          Err(err) => Poll::Ready(Err(err)),
        }
      })
      .boxed_local()
    }
  }

  impl ScriptedState {
    /// Gets the next thing that happens (ex. a key being pressed), which is
    /// nothing while the script waits for a draw after the provided draw
    /// count.
    fn next_event(&mut self, wait_draw_count: usize) -> Result<Option<Event>> {
      enum Next {
        Key(KeyEvent),
        Resize(TerminalSize),
        Waiting,
        StepDone,
      }

      loop {
        let draw_count = self.draw_count;
        let Some(step) = self.steps.front_mut() else {
          self.is_out_of_steps = true;
          bail!("A prompt is waiting for a key, but the script has no steps left.");
        };
        // starting a step is what ends the one before it
        let started_step_text = if step.is_started {
          None
        } else {
          step.is_started = true;
          Some(step.text.clone())
        };
        let next = match &mut step.kind {
          ScriptStepKind::Wait if wait_draw_count != draw_count => Next::StepDone,
          ScriptStepKind::Wait => Next::Waiting,
          ScriptStepKind::Resize(size) => Next::Resize(*size),
          ScriptStepKind::Keys(keys) => keys.pop_front().map(Next::Key).unwrap_or(Next::StepDone),
        };
        let is_step_done = match &step.kind {
          ScriptStepKind::Wait => matches!(next, Next::StepDone),
          ScriptStepKind::Resize(_) => true,
          ScriptStepKind::Keys(keys) => keys.is_empty(),
        };
        if let Some(text) = started_step_text {
          self.start_step(text);
        }
        if is_step_done {
          self.steps.pop_front();
        }
        match next {
          Next::Key(key) => return Ok(Some(Event::Key(key))),
          Next::Resize(size) => {
            self.size = Some(size);
            return Ok(Some(Event::Resize(size.cols, size.rows)));
          }
          Next::Waiting => return Ok(None),
          Next::StepDone => {}
        }
      }
    }

    fn start_step(&mut self, text: String) {
      let lines = self.current.as_ref().map(|screen| screen.lines.clone()).unwrap_or_default();
      self.screens.extend(self.current.take());
      // what's on the screen stays there until something draws over it
      self.current = Some(PromptScreen { step: text, lines });
    }
  }

  fn parse_step(text: &str) -> Result<ScriptStep> {
    let text = text.trim();
    let kind = if text == "wait" {
      ScriptStepKind::Wait
    } else if let Some(size) = text.strip_prefix("resize ") {
      let parsed_size = size
        .trim()
        .split_once('x')
        .and_then(|(cols, rows)| Some((cols.parse().ok()?, rows.parse().ok()?)));
      let Some((cols, rows)) = parsed_size else {
        bail!("Expected the size as <cols>x<rows> in step '{}'.", text);
      };
      ScriptStepKind::Resize(TerminalSize { cols, rows })
    } else if text.len() >= 2 && text.starts_with('"') && text.ends_with('"') {
      ScriptStepKind::Keys(
        text[1..text.len() - 1]
          .chars()
          .map(|c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
          .collect(),
      )
    } else {
      ScriptStepKind::Keys(VecDeque::from([parse_key(text)?]))
    };
    Ok(ScriptStep {
      text: text.to_string(),
      kind,
      is_started: false,
    })
  }

  fn parse_key(text: &str) -> Result<KeyEvent> {
    let mut modifiers = KeyModifiers::NONE;
    let mut parts = text.split('+').collect::<Vec<_>>();
    let name = parts.pop().unwrap_or_default();
    for part in parts {
      modifiers |= match part {
        "shift" => KeyModifiers::SHIFT,
        "alt" => KeyModifiers::ALT,
        "ctrl" => KeyModifiers::CONTROL,
        _ => bail!("Unknown modifier '{}' in step '{}'.", part, text),
      };
    }
    let code = match name {
      "up" => KeyCode::Up,
      "down" => KeyCode::Down,
      "left" => KeyCode::Left,
      "right" => KeyCode::Right,
      "enter" => KeyCode::Enter,
      "esc" => KeyCode::Esc,
      "tab" => KeyCode::Tab,
      "space" => KeyCode::Char(' '),
      "backspace" => KeyCode::Backspace,
      "delete" => KeyCode::Delete,
      "pageup" => KeyCode::PageUp,
      "pagedown" => KeyCode::PageDown,
      _ => {
        let mut chars = name.chars();
        match (chars.next(), chars.next()) {
          (Some(c), None) => KeyCode::Char(c),
          _ => bail!("Unknown key '{}' in step '{}'. Put text to type in quotes.", name, text),
        }
      }
    };
    Ok(KeyEvent::new(code, modifiers))
  }

  /// Renders the items the way the logger would to a terminal of the size.
  fn render_lines(text_items: &[LoggerTextItem], size: Option<TerminalSize>) -> Vec<String> {
    if text_items.is_empty() {
      return Vec::new();
    }
    let size = ConsoleSize {
      cols: size.map(|size| size.cols),
      rows: size.map(|size| size.rows),
    };
    let mut static_text = ConsoleStaticText::new(move || size);
    let text_items = text_items.iter().map(|item| item.as_static_text_item()).collect::<Vec<_>>();
    let text = static_text.render_items_with_size(text_items.iter(), size).unwrap_or_default();
    strip_ansi_codes(&text).split("\r\n").map(|line| line.trim_end().to_string()).collect()
  }

  mod test {
    use super::*;
    use pretty_assertions::assert_eq;

    fn text_item(text: &str) -> Vec<LoggerTextItem> {
      vec![LoggerTextItem::Text(text.to_string())]
    }

    /// Reads a key the script has ready without waiting.
    fn read_key(terminal: &ScriptedPromptTerminal) -> Result<Event> {
      terminal.read_key().now_or_never().unwrap()
    }

    #[test]
    fn keeps_the_screen_after_each_step() {
      let terminal = ScriptedPromptTerminal::new(&["down", "\"ab\"", "shift+up"], None).unwrap();
      terminal.draw(text_item("first"));
      assert_eq!(read_key(&terminal).unwrap(), Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)));
      terminal.draw(text_item("second"));
      // only what's shown once all the text is typed is kept
      assert_eq!(read_key(&terminal).unwrap(), Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE)));
      terminal.draw(text_item("third"));
      assert_eq!(read_key(&terminal).unwrap(), Event::Key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE)));
      terminal.draw(text_item("fourth"));
      assert_eq!(read_key(&terminal).unwrap(), Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT)));
      terminal.clear();
      assert!(terminal.remaining_steps().is_empty());
      assert!(!terminal.is_out_of_steps());
      assert_eq!(
        terminal.take_screens(),
        vec![
          PromptScreen {
            step: "start".to_string(),
            lines: vec!["first".to_string()],
          },
          PromptScreen {
            step: "down".to_string(),
            lines: vec!["second".to_string()],
          },
          PromptScreen {
            step: "\"ab\"".to_string(),
            lines: vec!["fourth".to_string()],
          },
          PromptScreen {
            step: "shift+up".to_string(),
            lines: vec![],
          },
        ]
      );
    }

    #[test]
    fn errors_when_out_of_steps_or_for_an_unknown_key() {
      let terminal = ScriptedPromptTerminal::new(&[], None).unwrap();
      assert_eq!(
        read_key(&terminal).unwrap_err().to_string(),
        "A prompt is waiting for a key, but the script has no steps left."
      );
      assert!(terminal.is_out_of_steps());
      assert_eq!(
        ScriptedPromptTerminal::new(&["nope"], None).err().unwrap().to_string(),
        "Unknown key 'nope' in step 'nope'. Put text to type in quotes."
      );
      assert_eq!(
        ScriptedPromptTerminal::new(&["super+a"], None).err().unwrap().to_string(),
        "Unknown modifier 'super' in step 'super+a'."
      );
    }

    #[test]
    fn resizes_the_terminal() {
      let terminal = ScriptedPromptTerminal::new(&["resize 20x5", "enter"], Some(TerminalSize { cols: 80, rows: 24 })).unwrap();
      assert_eq!(read_key(&terminal).unwrap(), Event::Resize(20, 5));
      assert_eq!(terminal.size(), Some(TerminalSize { cols: 20, rows: 5 }));
      assert_eq!(read_key(&terminal).unwrap(), Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
      assert_eq!(
        ScriptedPromptTerminal::new(&["resize big"], None).err().unwrap().to_string(),
        "Expected the size as <cols>x<rows> in step 'resize big'."
      );
    }

    #[test]
    fn wraps_and_cuts_off_lines_like_the_terminal() {
      let terminal = ScriptedPromptTerminal::new(&[], Some(TerminalSize { cols: 20, rows: 2 })).unwrap();
      terminal.draw(vec![
        LoggerTextItem::Text("title".to_string()),
        LoggerTextItem::HangingText {
          text: "alpha beta gamma delta epsilon zeta".to_string(),
          indent: 2,
        },
      ]);
      // the title doesn't fit in the two rows
      assert_eq!(
        terminal.take_screens()[0].lines,
        vec!["alpha beta gamma".to_string(), "  delta epsilon zeta".to_string()]
      );
    }
  }
}
