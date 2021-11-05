use crate::logging::Logger;
use crate::logging::LoggerRefreshItemKind;
use crate::logging::LoggerTextItem;
use crate::terminal::read_terminal_event;
use crate::types::ErrBox;
use crossterm::event::Event;
use crossterm::event::KeyCode;

pub fn show_confirm(logger: &Logger, context_name: &str, prompt: &str, default_value: bool) -> Result<bool, ErrBox> {
  let result = loop {
    let text_items = vec![LoggerTextItem::Text(format!(
      "{} ({}) \u{2588}", // show a cursor (block character)
      prompt,
      if default_value { "Y/n" } else { "y/N" }
    ))];
    logger.set_refresh_item(LoggerRefreshItemKind::Selection, text_items);

    match read_terminal_event()? {
      Event::Key(key_event) => match &key_event.code {
        KeyCode::Char(c) if *c == 'Y' || *c == 'y' => {
          break true;
        }
        KeyCode::Char(c) if *c == 'N' || *c == 'n' => {
          break false;
        }
        KeyCode::Enter => {
          break default_value;
        }
        KeyCode::Esc => {
          logger.remove_refresh_item(LoggerRefreshItemKind::Selection);
          return err!("Confirmation cancelled.");
        }
        _ => {}
      },
      _ => {
        // cause a refresh anyway
      }
    }
  };
  logger.remove_refresh_item(LoggerRefreshItemKind::Selection);

  logger.log_text_items(
    &vec![LoggerTextItem::Text(format!("{} {}", prompt, if result { "Y" } else { "N" }))],
    context_name,
    crate::terminal::get_terminal_width(),
  );

  Ok(result)
}
