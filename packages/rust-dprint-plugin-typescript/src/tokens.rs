use std::rc::Rc;
use super::*;
use swc_common::{BytePos};
use swc_ecma_parser::{token::{Token, TokenAndSpan}};

pub struct TokenFinder {
    pub tokens: Rc<Vec<TokenAndSpan>>, // todo: make this private
    token_index: usize,
}

impl TokenFinder {
    pub fn new(tokens: Rc<Vec<TokenAndSpan>>) -> TokenFinder {
        TokenFinder {
            tokens,
            token_index: 0,
        }
    }

    pub fn get_first_token_before<F>(&mut self, pos: BytePos, is_match: F) -> Option<TokenAndSpan> where F : Fn(&TokenAndSpan) -> bool{
        if self.tokens.is_empty() { return None; }

        self.move_to_node_pos(pos);

        loop {
            if !self.try_decrement_index() {
                return None;
            }

            let current_token = &self.tokens[self.token_index];
            if is_match(&current_token) {
                return Some(current_token.clone());
            }
        }
    }

    pub fn get_first_token_after<F>(&mut self, end: BytePos, is_match: F) -> Option<TokenAndSpan> where F : Fn(&TokenAndSpan) -> bool{
        if self.tokens.is_empty() { return None; }

        self.move_to_node_end(end);

        loop {
            if !self.try_increment_index() {
                return None;
            }

            let current_token = &self.tokens[self.token_index];
            if is_match(&current_token) {
                return Some(current_token.clone());
            }
        }
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
}