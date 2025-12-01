use anyhow::Result;
use anyhow::bail;
use crossterm::event::Event;
use crossterm::event::KeyCode;

use super::Logger;
use super::LoggerRefreshItemKind;
use super::LoggerTextItem;
use crate::utils::terminal::read_terminal_key_press;

pub fn show_confirm(logger: &Logger, context_name: &str, prompt: &str, default_value: bool) -> Result<bool> {
  let result = loop {
    let text_items = vec![LoggerTextItem::Text(format!(
      "{} ({}) \u{2588}", // show a cursor (block character)
      prompt,
      if default_value { "Y/n" } else { "y/N" }
    ))];
    logger.set_refresh_item(LoggerRefreshItemKind::Selection, text_items);

    if let Event::Key(key_event) = read_terminal_key_press()? {
      match &key_event.code {
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
          bail!("Confirmation cancelled.");
        }
        _ => {}
      }
    } else {
      // cause a refresh anyway
    }
  };
  logger.remove_refresh_item(LoggerRefreshItemKind::Selection);

  logger.log_text_items(&[LoggerTextItem::Text(format!("{} {}", prompt, if result { "Y" } else { "N" }))], context_name);

  Ok(result)
}
