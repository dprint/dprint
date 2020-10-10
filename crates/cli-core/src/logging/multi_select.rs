use crate::logging::{Logger, LoggerRefreshItemKind};
use crate::types::ErrBox;
use crossterm::event::{read, Event, KeyCode};

struct MultiSelectData<'a> {
    prompt: &'a str,
    items: Vec<(bool, &'a String)>,
    active_index: usize,
}

pub fn show_multi_select(logger: &Logger, context_name: &str, prompt: &str, items: Vec<(bool, &String)>) -> Result<Vec<usize>, ErrBox> {
    let mut data = MultiSelectData {
        prompt,
        items,
        active_index: 0,
    };

    loop {
        let text = render_multi_select(&data);
        logger.set_refresh_item(LoggerRefreshItemKind::Selection, text);

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
                    _ => {

                    }
                }
            },
            _ => {
                // cause a refresh anyway?
            }
        }
    }
    logger.remove_refresh_item(LoggerRefreshItemKind::Selection);

    let mut result = Vec::new();
    if data.items.iter().any(|(is_selected, _)| *is_selected) {
        let mut text = String::new();
        text.push_str(&data.prompt);
        for (i, (is_selected, item_text)) in data.items.iter().enumerate() {
            if *is_selected {
                // todo: handle text wrapping
                text.push_str(&format!("\n * {}", item_text));
                result.push(i);
            }
        }
        logger.log(&text, context_name);
    }

    Ok(result)
}

fn render_multi_select(data: &MultiSelectData) -> String {
    let mut text = String::new();
    text.push_str(&data.prompt);

    for (i, (is_selected, item_text)) in data.items.iter().enumerate() {
        text.push_str("\n");
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

        // todo: handle text wrapping
        text.push_str(item_text);
    }

    text
}
