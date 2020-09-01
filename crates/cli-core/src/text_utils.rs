/// This is necessary because Indicatif only supports messages on one line. If the lines span
/// multiple lines then issue #278 occurs.
///
/// This takes a text like "Downloading " and "https://dprint.dev/somelongurl"
/// and may truncate it to "Downloading https://dprint.dev...longurl"
pub fn get_middle_truncted_text(prompt: &str, text: &str) -> String {
    // For some reason, the "console" crate was not correctly returning
    // the terminal size, so ended up using the terminal_size crate directly
    use terminal_size::{terminal_size, Width};

    let term_width = if let Some((Width(width), _)) = terminal_size() {
        width as usize
    } else {
        return format!("{}{}", prompt, text);
    };

    let prompt_width = console::measure_text_width(prompt);
    let text_width = console::measure_text_width(text);
    let is_text_within_term_width = prompt_width + text_width < term_width;
    let should_give_up = term_width < prompt_width || (term_width - prompt_width) / 2 < 3;

    if is_text_within_term_width || should_give_up {
        format!("{}{}", prompt, text)
    } else {
        let middle_point = (term_width - prompt_width) / 2;
        let text_chars = text.chars().collect::<Vec<_>>();
        let first_text: String = (&text_chars[0..middle_point - 2]).iter().collect();
        let second_text: String = (&text_chars[text_chars.len() - middle_point + 1..]).iter().collect();
        let text = format!("{}...{}", first_text, second_text);
        format!("{}{}", prompt, text)
    }
}
