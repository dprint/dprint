use super::super::{ParseError, ast_nodes::{Range}};
use super::char_scanner::CharScanner;

pub fn parse_text_in_brackets(start_pos: usize, char_scanner: &mut CharScanner) -> Result<String, ParseError> {
    parse_text_in_container(start_pos, char_scanner, '[', ']')
}

pub fn parse_text_in_parens(start_pos: usize, char_scanner: &mut CharScanner) -> Result<String, ParseError> {
    parse_text_in_container(start_pos, char_scanner, '(', ')')
}

pub fn parse_text_in_angle_brackets(start_pos: usize, char_scanner: &mut CharScanner) -> Result<String, ParseError> {
    parse_text_in_container(start_pos, char_scanner, '<', '>')
}

fn parse_text_in_container(start_pos: usize, char_scanner: &mut CharScanner, open_char: char, close_char: char) -> Result<String, ParseError> {
    let mut text = String::new();
    while let Some((byte_pos, c)) = char_scanner.next() {
        if c == close_char {
            return Ok(text)
        } else if c == open_char {
            return Err(ParseError::new(
                Range { start: byte_pos, end: byte_pos },
                &format!("Unexpected open container char `{}`.", open_char)
            ));
        } else {
            text.push(c);
        }
    }

    return Err(ParseError::new(
        Range { start: start_pos, end: char_scanner.pos() },
        &format!("Did not find container close char `{}`.", close_char)
    ));
}

pub fn parse_link_url_and_title(text: &str) -> (String, Option<String>) {
    let mut char_scanner = CharScanner::new(0, text);
    let mut url = String::new();
    let mut title: Option<String> = None;

    char_scanner.skip_spaces();

    while let Some((_, c)) = char_scanner.next() {
        match c {
            '"' => match try_parse_title(&mut char_scanner) {
                Ok(text) => title = Some(text),
                Err(text) => {
                    url.push('"');
                    url.push_str(&text);
                },
            },
            _ => url.push(c),
        }
    }

    (url, title)
}

fn try_parse_title(char_scanner: &mut CharScanner) -> Result<String, String> {
    let mut text = String::new();

    while let Some((_, c)) = char_scanner.next() {
        match c {
            '"' => {
                if char_scanner.peek().is_some() {
                    text.push('"');
                    return Err(text);
                }
                return Ok(text)
            },
            _ => text.push(c),
        }
    }

    Err(text)
}
