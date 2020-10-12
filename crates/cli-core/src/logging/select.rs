use crate::logging::{Logger, LoggerRefreshItemKind, LoggerTextItem};
use crate::types::ErrBox;
use crossterm::event::{read, Event, KeyCode};

struct SelectData<'a> {
    prompt: &'a str,
    item_hanging_indent: u16,
    items: &'a Vec<String>,
    active_index: usize,
}

pub fn show_select(
    logger: &Logger,
    context_name: &str,
    prompt: &str,
    item_hanging_indent: u16,
    items: &Vec<String>
) -> Result<usize, ErrBox> {
    let mut data = SelectData {
        prompt,
        item_hanging_indent,
        items,
        active_index: 0,
    };

    loop {
        let text_items = render_select(&data);
        logger.set_refresh_item(LoggerRefreshItemKind::Selection, text_items);

        match read()? {
            Event::Key(key_event) => {
                match &key_event.code {
                    KeyCode::Up => {
                        if data.active_index == 0 {
                            data.active_index = data.items.len() - 1;
                        } else {
                            data.active_index -= 1;
                        }
                    },
                    KeyCode::Down => {
                        data.active_index = (data.active_index + 1) % data.items.len();
                    },
                    KeyCode::Enter => {
                        break;
                    },
                    KeyCode::Esc => {
                        logger.remove_refresh_item(LoggerRefreshItemKind::Selection);
                        return err!("Selection cancelled.");
                    }
                    _ => {}
                }
            },
            _ => {
                // cause a refresh anyway
            }
        }
    }
    logger.remove_refresh_item(LoggerRefreshItemKind::Selection);

    logger.log_text_items(&vec![
        LoggerTextItem::Text(data.prompt.to_string()),
        LoggerTextItem::HangingText {
            text: data.items[data.active_index].to_string(),
            indent: item_hanging_indent,
        }
    ], context_name, crate::terminal::get_terminal_width());

    Ok(data.active_index)
}

fn render_select(data: &SelectData) -> Vec<LoggerTextItem> {
    let mut result = Vec::new();
    result.push(LoggerTextItem::Text(data.prompt.to_string()));

    for (i, item_text) in data.items.iter().enumerate() {
        let mut text = String::new();
        text.push_str(if i == data.active_index {
            ">"
        } else {
            " "
        });
        text.push_str(" ");
        text.push_str(item_text);
        result.push(LoggerTextItem::HangingText {
            text,
            indent: 2 + data.item_hanging_indent,
        });
    }

    result
}
