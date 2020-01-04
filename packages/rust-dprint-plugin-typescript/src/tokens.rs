use std::str;
use std::rc::Rc;
use super::*;
use swc_common::{BytePos, SpanData};
use swc_ecma_parser::{token::{Token, TokenAndSpan, Word, Keyword, BinOpToken}};

pub struct TokenFinder {
    tokens: Rc<Vec<TokenAndSpan>>,
    file_bytes: Rc<Vec<u8>>,
    token_index: usize,
}

impl TokenFinder {
    pub fn new(tokens: Rc<Vec<TokenAndSpan>>, file_bytes: Rc<Vec<u8>>) -> TokenFinder {
        TokenFinder {
            tokens,
            file_bytes,
            token_index: 0,
        }
    }

    pub fn get_token_at(&mut self, node: &dyn Ranged) -> Option<TokenAndSpan> {
        let pos = node.lo();
        if self.tokens.is_empty() { return None; }
        self.move_to_node_pos(pos);
        let current_token = &self.tokens[self.token_index];
        if current_token.lo() == pos {
            return Some(current_token.clone())
        }

        None
    }

    pub fn get_first_open_paren_token_before(&mut self, node: &dyn Ranged) -> Option<TokenAndSpan> {
        self.get_first_token_before_equaling_token(node, Token::LParen)
    }

    pub fn get_first_angle_bracket_token_before(&mut self, node: &dyn Ranged) -> Option<TokenAndSpan> {
        self.get_first_token_before_equaling_token(node, Token::BinOp(BinOpToken::Lt))
    }

    pub fn get_first_open_brace_token_before(&mut self, node: &dyn Ranged) -> Option<TokenAndSpan> {
        self.get_first_token_before_equaling_token(node, Token::LBrace)
    }

    fn get_first_token_before_equaling_token(&mut self, node: &dyn Ranged, searching_token: Token) -> Option<TokenAndSpan> {
        self.get_first_token_before(node.lo(), |token| token.token == searching_token)
    }

    pub fn get_first_non_comment_token_before(&mut self, node: &dyn Ranged) -> Option<TokenAndSpan> {
        self.get_first_token_before(node.lo(), |_| true)
    }

    pub fn get_first_open_paren_token_within(&mut self, node: &dyn Ranged) -> Option<TokenAndSpan> {
        self.get_first_token_within(node, |token| token.token == Token::LParen)
    }

    pub fn get_first_open_brace_token_within(&mut self, node: &dyn Ranged) -> Option<TokenAndSpan> {
        self.get_first_token_within(node, |token| token.token == Token::LBrace)
    }

    pub fn get_first_close_brace_token_before(&mut self, node: &dyn Ranged) -> Option<TokenAndSpan> {
        self.get_first_token_before(node.lo(), |token|token.token == Token::RBrace)
    }

    pub fn get_first_else_keyword_before(&mut self, node: &dyn Ranged) -> Option<TokenAndSpan> {
        self.get_first_token_before(node.lo(), |token|token.token == Token::Word(Word::Keyword(Keyword::Else)))
    }

    pub fn get_first_colon_token_after(&mut self, node: &dyn Ranged) -> Option<TokenAndSpan> {
        self.get_first_token_after(node.hi(), |token|token.token == Token::Colon)
    }

    pub fn get_first_open_bracket_token_within(&mut self, node: &dyn Ranged) -> Option<TokenAndSpan> {
        self.get_first_token_within(node, |token| token.token == Token::LBracket)
    }

    pub fn get_first_comma_within(&mut self, node: &dyn Ranged) -> Option<TokenAndSpan> {
        self.get_first_token_within(node, |token| token.token == Token::Comma)
    }

    pub fn get_first_semi_colon_within(&mut self, node: &dyn Ranged) -> Option<TokenAndSpan> {
        self.get_first_token_within(node, |token| token.token == Token::Semi)
    }

    pub fn get_first_token_before_with_text(&mut self, node: &dyn Ranged, text: &str) -> Option<TokenAndSpan> {
        let file_bytes = self.file_bytes.clone();
        self.get_first_token_before(node.lo(), |token| get_text(&file_bytes, &token.span.data()) == text)
    }

    pub fn get_first_token_within_with_text(&mut self, node: &dyn Ranged, text: &str) -> Option<TokenAndSpan> {
        let file_bytes = self.file_bytes.clone();
        self.get_first_token_within(node, |token| get_text(&file_bytes, &token.span.data()) == text)
    }

    pub fn get_first_token_after_with_text(&mut self, node: &dyn Ranged, searching_token_text: &str) -> Option<TokenAndSpan> {
        let file_bytes = self.file_bytes.clone();
        self.get_first_token_after(node.hi(), |token| get_text(&file_bytes, &token.span.data()) == searching_token_text)
    }

    pub fn get_token_text_at_pos(&mut self, pos: BytePos) -> Option<&str> {
        if self.tokens.is_empty() { return None; }
        self.move_to_node_pos(pos);
        let span_data = self.tokens[self.token_index].span.data();
        if span_data.lo == pos {
            return Some(self.get_text(&span_data));
        }

        None
    }

    pub fn get_first_token_before<F>(&mut self, pos: BytePos, is_match: F) -> Option<TokenAndSpan> where F : Fn(&TokenAndSpan) -> bool{
        if self.tokens.is_empty() { return None; }
        self.move_to_node_pos(pos);

        if self.tokens[self.token_index].lo() < pos {
            let current_token = &self.tokens[self.token_index];
            if is_match(&current_token) {
                return Some(current_token.clone());
            }
        }

        while self.try_decrement_index() {
            let current_token = &self.tokens[self.token_index];
            if is_match(&current_token) {
                return Some(current_token.clone());
            }
        }

        return None;
    }

    pub fn get_first_token_after<F>(&mut self, end: BytePos, is_match: F) -> Option<TokenAndSpan> where F : Fn(&TokenAndSpan) -> bool {
        if self.tokens.is_empty() { return None; }
        self.move_to_node_end(end);

        while self.try_increment_index() {
            let current_token = &self.tokens[self.token_index];
            if is_match(&current_token) {
                return Some(current_token.clone());
            }
        }

        return None;
    }

    pub fn get_first_token_within<F>(&mut self, node: &dyn Ranged, is_match: F) -> Option<TokenAndSpan> where F : Fn(&TokenAndSpan) -> bool {
        let node_span_data = node.span().data();
        let pos = node_span_data.lo;
        let end = node_span_data.hi;
        if self.tokens.is_empty() { return None; }
        self.move_to_node_pos(pos);

        while self.try_increment_index() {
            let current_token = &self.tokens[self.token_index];
            let token_pos = current_token.span.data().lo;
            if token_pos >= end {
                break;
            } else if is_match(&current_token) {
                return Some(current_token.clone());
            }
        }

        None
    }

    fn move_to_node_pos(&mut self, pos: BytePos) {
        while self.tokens[self.token_index].lo() < pos {
            if !self.try_increment_index() {
                break;
            }
        }

        while self.tokens[self.token_index].lo() > pos {
            if !self.try_decrement_index() {
                break;
            }
        }
    }

    fn move_to_node_end(&mut self, end: BytePos) {
        while self.tokens[self.token_index].hi() < end {
            if !self.try_increment_index() {
                break;
            }
        }

        while self.tokens[self.token_index].hi() > end {
            if !self.try_decrement_index() {
                break;
            }
        }
    }

    fn try_increment_index(&mut self) -> bool {
        if self.token_index == self.tokens.len() - 1 {
            false
        } else {
            self.token_index += 1;
            true
        }
    }

    fn try_decrement_index(&mut self) -> bool {
        if self.token_index == 0 {
            false
        } else {
            self.token_index -= 1;
            true
        }
    }

    fn get_text(&self, span_data: &SpanData) -> &str {
        get_text(&self.file_bytes, span_data)
    }
}

fn get_text<'a>(file_bytes: &'a Rc<Vec<u8>>, span_data: &SpanData) -> &'a str {
    let bytes = &file_bytes[(span_data.lo.0 as usize)..(span_data.hi.0 as usize)];
    str::from_utf8(&bytes).unwrap()
}
