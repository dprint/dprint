use anyhow::Result;
use anyhow::bail;
use crossterm::event::Event;
use crossterm::event::KeyCode;

use super::Logger;
use super::LoggerRefreshItemKind;
use super::LoggerTextItem;
use crate::utils::terminal::read_terminal_key_press;

pub trait ShowConfirmStrategy {
  fn render(&self, selected: Option<bool>) -> String;
  fn default_value(&self) -> bool;
}

pub struct BasicShowConfirmStrategy<'a> {
  pub prompt: &'a str,
  pub default_value: bool,
}

impl ShowConfirmStrategy for BasicShowConfirmStrategy<'_> {
  fn render(&self, selected: Option<bool>) -> String {
    match selected {
      Some(value) => {
        format!("{} {}", self.prompt, if value { "Y" } else { "N" })
      }
      None => {
        format!(
          "{} ({}) \u{2588}", // show a cursor (block character)
          self.prompt,
          if self.default_value { "Y/n" } else { "y/N" }
        )
      }
    }
  }

  fn default_value(&self) -> bool {
    self.default_value
  }
}

pub fn show_confirm(logger: &Logger, context_name: &str, strategy: &dyn ShowConfirmStrategy) -> Result<bool> {
  let result = loop {
    let text_items = vec![LoggerTextItem::Text(strategy.render(None))];
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
          break strategy.default_value();
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

  logger.log_text_items(&[LoggerTextItem::Text(strategy.render(Some(result)))], context_name);

  Ok(result)
}
