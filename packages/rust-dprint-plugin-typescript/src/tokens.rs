use std::str;
use std::rc::Rc;
use super::*;
use swc_common::{BytePos, Span};

pub struct TokenFinder {
    token_parser: TokenParser,
    file_bytes: Rc<Vec<u8>>,
}

impl TokenFinder {
    pub fn new(file_bytes: Rc<Vec<u8>>) -> TokenFinder {
        let token_parser = TokenParser::new(file_bytes.clone());
        TokenFinder {
            token_parser,
            file_bytes,
        }
    }

    // done

    pub fn get_char_at(&self, pos: &BytePos) -> char {
        self.file_bytes[pos.0 as usize] as char
    }

    pub fn get_first_open_paren_token_within(&self, node: &dyn Ranged) -> Option<Span> {
        self.get_token_within_from_parser(node, "(")
    }

    pub fn get_first_open_brace_token_within(&self, node: &dyn Ranged) -> Option<Span> {
        self.get_token_within_from_parser(node, "{")
    }

    pub fn get_first_open_bracket_token_within(&self, node: &dyn Ranged) -> Option<Span> {
        self.get_token_within_from_parser(node, "[")
    }

    pub fn get_first_comma_within(&self, node: &dyn Ranged) -> Option<Span> {
        self.get_token_within_from_parser(node, ",")
    }

    pub fn get_first_semi_colon_within(&self, node: &dyn Ranged) -> Option<Span> {
        self.get_token_within_from_parser(node, ";")
    }

    fn get_token_within_from_parser(&self, node: &dyn Ranged, text: &str) -> Option<Span> {
        let span_data = node.span().data();
        self.token_parser.get_next_token_with_text(span_data.lo, Some(span_data.hi), false, text)
    }

    pub fn get_first_colon_token_after(&self, node: &dyn Ranged) -> Option<Span> {
        self.get_token_after_from_parser(node, ":")
    }

    pub fn get_first_comma_after(&self, node: &dyn Ranged) -> Option<Span> {
        self.get_token_after_from_parser(node, ",")
    }

    pub fn get_first_operator_after_with_text(&self, node: &dyn Ranged, operator_text: &str) -> Option<Span> {
        self.get_token_after_from_parser(node, operator_text)
    }

    fn get_token_after_from_parser(&self, node: &dyn Ranged, text: &str) -> Option<Span> {
        self.token_parser.get_next_token_with_text(node.hi(), None, false, text)
    }

    pub fn get_first_keyword_after(&self, node: &dyn Ranged, keyword_text: &str) -> Option<Span> {
        self.token_parser.get_next_token_with_text(node.hi(), None, true, keyword_text)
    }

    pub fn get_first_else_keyword_within(&self, node: &dyn Ranged) -> Option<Span> {
        let span_data = node.span().data();
        self.token_parser.get_next_token_with_text(span_data.lo, Some(span_data.hi), true, "else")
    }

    pub fn get_first_open_paren_token_before(&self, node: &dyn Ranged) -> Option<Span> {
        self.get_token_before_from_parser(node, "(")
    }

    pub fn get_first_open_brace_token_before(&self, node: &dyn Ranged) -> Option<Span> {
        self.get_token_before_from_parser(node, "{")
    }

    fn get_token_before_from_parser(&self, node: &dyn Ranged, text: &str) -> Option<Span> {
        self.token_parser.get_previous_token_with_text(None, node.lo(), false, text)
    }

    pub fn get_previous_token_pos_before(&self, node: &dyn Ranged) -> BytePos {
        self.token_parser.get_previous_token_pos(node.lo())
    }

    pub fn get_next_token_pos_after(&self, node: &dyn Ranged) -> BytePos {
        self.token_parser.get_next_token_pos(node.hi())
    }
}

pub struct TokenParser {
    file_bytes: Rc<Vec<u8>>,
}

// This token parser makes a lot of assumptions
// todo: Maybe instead of handling strings the search ranges could be limited?

impl TokenParser {
    fn new(file_bytes: Rc<Vec<u8>>) -> TokenParser {
        TokenParser {
            file_bytes,
        }
    }

    pub fn get_previous_token_pos(&self, end: BytePos) -> BytePos {
        let mut pos = end.0 as usize;
        let FORWARD_SLASH = '/' as u8;
        let STAR = '*' as u8;
        let NEW_LINE = '\n' as u8;
        let mut is_in_block_comment = false;

        if pos == 0 {
            return BytePos(0);
        } else {
            pos -= 1;
        }

        loop {
            let next_char = if pos == 0 { None } else { Some(&self.file_bytes[pos - 1]) };
            let current_char = &self.file_bytes[pos];

            if is_in_block_comment {
                if next_char == Some(&FORWARD_SLASH) && current_char == &STAR {
                    is_in_block_comment = false;
                    if pos <= 1 { break; }
                    pos -= 2;
                } else {
                    if pos == 0 { break; }
                    pos -= 1;
                }
                continue;
            } else if next_char == Some(&STAR) && current_char == &FORWARD_SLASH {
                is_in_block_comment = true;
                if pos <= 1 { break; }
                pos -= 2;
                continue;
            }

            if !(current_char.to_owned() as char).is_whitespace() {
                if let Some(comment_start) = self.get_previous_line_comment_start_on_line(pos) {
                    pos = comment_start;
                } else {
                    return BytePos((pos as u32) + 1);
                }
            }

            if pos == 0 {
                break;
            } else {
                pos -= 1;
            }
        }

        BytePos(0)
    }

    pub fn get_previous_token_with_text(&self, start: Option<BytePos>, end: BytePos, non_alpha_numeric_surrounding: bool, text: &str) -> Option<Span> {
        // todo: should handle strings
        let start = start.map(|x| x.0 as usize).unwrap_or(0);
        let mut pos = end.0 as usize;
        let FORWARD_SLASH = '/' as u8;
        let STAR = '*' as u8;
        let mut is_in_block_comment = false;
        let text_bytes = text.as_bytes();

        if pos == 0 {
            return None;
        } else {
            pos -= 1;
        }

        while pos >= start {
            let next_char = if pos == 0 { None } else { Some(&self.file_bytes[pos - 1]) };
            let current_char = &self.file_bytes[pos];

            if is_in_block_comment {
                if next_char == Some(&FORWARD_SLASH) && current_char == &STAR {
                    is_in_block_comment = false;
                    if pos <= 1 { break; }
                    pos -= 2;
                } else {
                    if pos == 0 { break; }
                    pos -= 1;
                }
                continue;
            } else if next_char == Some(&STAR) && current_char == &FORWARD_SLASH {
                is_in_block_comment = true;
                if pos <= 1 { break; }
                pos -= 2;
                continue;
            }

            let end_pos = pos + text_bytes.len();
            if text_bytes == &self.file_bytes[pos..end_pos] {
                if !non_alpha_numeric_surrounding || !is_alpha_numeric(self.file_bytes.get(pos - 1)) && !is_alpha_numeric(self.file_bytes.get(end_pos)) {
                    if let Some(comment_start) = self.get_previous_line_comment_start_on_line(pos) {
                        pos = comment_start;
                    } else {
                        return Some(Span::new(BytePos(pos as u32), BytePos((pos + text.len()) as u32), Default::default()));
                    }
                }
            }

            if pos == 0 {
                break;
            } else {
                pos -= 1;
            }
        }

        None
    }

    fn get_previous_line_comment_start_on_line(&self, pos: usize) -> Option<usize> {
        // todo: should handle strings
        let FORWARD_SLASH = '/' as u8;
        let NEW_LINE = '\n' as u8;
        let mut pos = pos;
        while pos >= 1 {
            let next_char = &self.file_bytes[pos - 1];
            let current_char = &self.file_bytes[pos];

            if next_char == &FORWARD_SLASH && current_char == &FORWARD_SLASH {
                return Some(pos - 1);
            }
            if current_char == &NEW_LINE {
                return None;
            }

            pos -= 1;
        }
        return None;
    }

    pub fn get_next_token_with_text(&self, pos: BytePos, end: Option<BytePos>, non_alpha_numeric_surrounding: bool, text: &str) -> Option<Span> {
        // todo: should handle strings
        let mut pos = pos.0 as usize;
        let end = end.map(|x| x.0 as usize).unwrap_or(self.file_bytes.len());
        let FORWARD_SLASH = '/' as u8;
        let STAR = '*' as u8;
        let NEW_LINE = '\n' as u8;
        let mut is_in_line_comment = false;
        let mut is_in_block_comment = false;
        let text_bytes = text.as_bytes();

        while pos < end {
            let current_char = &self.file_bytes[pos];
            let next_char = self.file_bytes.get(pos + 1);

            if is_in_line_comment {
                if current_char == &NEW_LINE {
                    is_in_line_comment = false;
                }
                pos += 1;
                continue;
            } else if is_in_block_comment {
                if current_char == &STAR && next_char == Some(&FORWARD_SLASH) {
                    is_in_block_comment = false;
                    pos += 2;
                } else {
                    pos += 1;
                }
                continue;
            }

            if current_char == &FORWARD_SLASH && next_char == Some(&FORWARD_SLASH) {
                is_in_line_comment = true;
                pos += 1;
                continue;
            }

            if current_char == &FORWARD_SLASH && next_char == Some(&STAR) {
                is_in_block_comment = true;
                pos += 2;
                continue;
            }


            let end_pos = pos + text_bytes.len();
            if end_pos > self.file_bytes.len() {
                return None;
            }

            if text_bytes == &self.file_bytes[pos..end_pos] {
                if !non_alpha_numeric_surrounding || !is_alpha_numeric(self.file_bytes.get(pos - 1)) && !is_alpha_numeric(self.file_bytes.get(end_pos)) {
                    return Some(Span::new(BytePos(pos as u32), BytePos((pos + text.len()) as u32), Default::default()));
                }
            }

            pos += 1;
        }

        return None;
    }

    pub fn get_next_token_pos(&self, pos: BytePos) -> BytePos {
        let mut pos = pos.0 as usize;
        let end = self.file_bytes.len();
        let FORWARD_SLASH = '/' as u8;
        let STAR = '*' as u8;
        let NEW_LINE = '\n' as u8;
        let mut is_in_line_comment = false;
        let mut is_in_block_comment = false;

        while pos < end {
            let current_char = &self.file_bytes[pos];
            let next_char = self.file_bytes.get(pos + 1);

            if is_in_line_comment {
                if current_char == &NEW_LINE {
                    is_in_line_comment = false;
                }
                pos += 1;
                continue;
            } else if is_in_block_comment {
                if current_char == &STAR && next_char == Some(&FORWARD_SLASH) {
                    is_in_block_comment = false;
                    pos += 2;
                } else {
                    pos += 1;
                }
                continue;
            }

            if current_char == &FORWARD_SLASH && next_char == Some(&FORWARD_SLASH) {
                is_in_line_comment = true;
                pos += 1;
                continue;
            }

            if current_char == &FORWARD_SLASH && next_char == Some(&STAR) {
                is_in_block_comment = true;
                pos += 2;
                continue;
            }

            if !(current_char.to_owned() as char).is_whitespace() {
                return BytePos(pos as u32);
            }

            pos += 1;
        }

        return BytePos(end as u32);
    }
}


fn is_alpha_numeric(value: Option<&u8>) -> bool {
    if let Some(value) = value {
        let c = value.to_owned() as char;
        return c.is_alphanumeric();
    } else {
        return false;
    }
}
