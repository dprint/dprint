use std::str::CharIndices;
use super::super::{ParseError, ast_nodes::{Range}};

pub struct CharScanner<'a> {
    char_indices: CharIndices<'a>,
    offset: usize,
    pos: usize,
    previous: Option<(usize, char)>,
    current: Option<(usize, char)>,
    next: Option<(usize, char)>,
}

impl<'a> CharScanner<'a> {
    pub fn new(offset: usize, text: &'a str) -> CharScanner<'a> {
        let char_indices = text.char_indices();
        let mut scanner = CharScanner {
            char_indices,
            offset,
            pos: offset,
            previous: None,
            current: None,
            next: None,
        };

        scanner.next(); // initialize
        scanner
    }

    pub fn save_state(&self) -> CharScanner<'a> {
        CharScanner {
            char_indices: self.char_indices.clone(),
            offset: self.offset,
            pos: self.pos,
            previous: self.previous,
            current: self.current,
            next: self.next,
        }
    }

    pub fn assert_char(&mut self, searching_char: char) -> Result<(), ParseError> {
        while let Some((byte_pos, c)) = self.next() {
            if c == searching_char {
                return Ok(());
            } else if c.is_whitespace() {
                continue;
            } else {
                return Err(ParseError::new(
                    Range { start: byte_pos, end: byte_pos },
                    &format!("Unexpected token `{}` when expected `{}`.", c, searching_char)
                ));
            }
        }

        return Err(ParseError::new(
            Range { start: self.pos, end: self.pos},
            &format!("Did not find expected char of `{}`", searching_char)
        ));
    }

    pub fn skip_spaces(&mut self) {
        loop {
            if let Some((_, c)) = &self.next {
                if c == &'\n' || !c.is_whitespace() {
                    break;
                }

                self.next();
            } else {
                break;
            }
        }
    }

    pub fn next(&mut self) -> Option<(usize, char)> {
        self.previous = self.current;
        self.current = self.next;
        self.next = self.char_indices.next().map(|(byte_pos, c)| (byte_pos + self.offset, c));

        if let Some(current) = self.current {
            self.pos = current.0;
        } else if let Some(previous) = self.previous {
            self.pos += previous.1.len_utf8();
        }

        self.current
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn previous(&self) -> Option<(usize, char)> {
        self.previous
    }

    pub fn current(&self) -> Option<(usize, char)> {
        self.current
    }

    pub fn peek(&self) -> Option<(usize, char)> {
        self.next
    }
}
