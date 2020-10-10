use crate::logging::{Logger, LoggerRefreshItemKind};
use crate::types::ErrBox;
use crossterm::event::{read, Event, KeyCode};

struct SelectData<'a> {
    prompt: &'a str,
    items: &'a Vec<String>,
    active_index: usize,
}

pub fn show_select(logger: &Logger, context_name: &str, prompt: &str, items: &Vec<String>) -> Result<usize, ErrBox> {
    let mut data = SelectData {
        prompt,
        items,
        active_index: 0,
    };

    loop {
        let text = render_select(&data);
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

            }
        }
    }
    logger.remove_refresh_item(LoggerRefreshItemKind::Selection);

    logger.log(&format!("{}\n  {}", data.prompt, data.items[data.active_index]), context_name);

    Ok(data.active_index)
}

fn render_select(data: &SelectData) -> String {
    let mut text = String::new();
    text.push_str(&data.prompt);

    for (i, item_text) in data.items.iter().enumerate() {
        text.push_str("\n");
        text.push_str(if i == data.active_index {
            ">"
        } else {
            " "
        });
        text.push_str(" ");

        // todo: handle text wrapping
        text.push_str(item_text);
    }

    text
}
