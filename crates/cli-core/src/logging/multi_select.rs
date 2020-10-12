use crate::logging::{Logger, LoggerRefreshItemKind, LoggerTextItem};
use crate::types::ErrBox;
use crossterm::event::{read, Event, KeyCode};

struct MultiSelectData<'a> {
    prompt: &'a str,
    item_hanging_indent: u16,
    items: Vec<(bool, &'a String)>,
    active_index: usize,
}

pub fn show_multi_select(
    logger: &Logger,
    context_name: &str,
    prompt: &str,
    item_hanging_indent: u16,
    items: Vec<(bool, &String)>
) -> Result<Vec<usize>, ErrBox> {
    let mut data = MultiSelectData {
        prompt,
        items,
        item_hanging_indent,
        active_index: 0,
    };

    loop {
        let text_items = render_multi_select(&data);
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
                    KeyCode::Char(' ') => {
                        // select an item
                        let mut current_item = data.items.get_mut(data.active_index).unwrap();
                        current_item.0 = !current_item.0;
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

    logger.log_text_items(&render_complete(&data), context_name, crate::terminal::get_terminal_width());

    // return the selected indexes
    let mut result = Vec::new();
    for (i, (is_selected, _)) in data.items.iter().enumerate() {
        if *is_selected { result.push(i); }
    }
    Ok(result)
}

fn render_multi_select(data: &MultiSelectData) -> Vec<LoggerTextItem> {
    let mut result = Vec::new();
    result.push(LoggerTextItem::Text(data.prompt.to_string()));

    for (i, (is_selected, item_text)) in data.items.iter().enumerate() {
        let mut text = String::new();
        text.push_str(if i == data.active_index {
            ">"
        } else {
            " "
        });
        text.push_str(" [");
        text.push_str(if *is_selected {
            "x"
        } else {
            " "
        });
        text.push_str("] ");
        text.push_str(item_text);

        result.push(LoggerTextItem::HangingText {
            text,
            indent: 7 + data.item_hanging_indent,
        });
    }

    result
}

fn render_complete(data: &MultiSelectData) -> Vec<LoggerTextItem> {
    let mut result = Vec::new();
    if data.items.iter().any(|(is_selected, _)| *is_selected) {
        result.push(LoggerTextItem::Text(data.prompt.to_string()));
        for (is_selected, item_text) in data.items.iter() {
            if *is_selected {
                result.push(LoggerTextItem::HangingText {
                    text: format!(" * {}", item_text),
                    indent: 3 + data.item_hanging_indent,
                });
            }
        }
    }
    result
}
