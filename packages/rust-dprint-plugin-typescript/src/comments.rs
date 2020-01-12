use std::rc::Rc;
use std::collections::HashMap;
use super::*;
use swc_common::{BytePos, comments::{Comments, Comment, CommentMap}};
use swc_ecma_parser::{token::{Token, TokenAndSpan}};

pub struct CommentCollection {
    leading: HashMap<BytePos, Vec<Comment>>,
    trailing: HashMap<BytePos, Vec<Comment>>,
    token_finder: TokenFinder,
    file_bytes: Rc<Vec<u8>>,
    tokens: Rc<Vec<TokenAndSpan>>,
    token_index: usize,
}

impl CommentCollection {
    pub fn new(comments: Comments, tokens: Rc<Vec<TokenAndSpan>>, file_bytes: Rc<Vec<u8>>) -> CommentCollection {
        let (leading, trailing) = comments.take_all();
        // println!("Leading: {:?}", leading);
        // println!("Trailing: {:?}", trailing);
        CommentCollection {
            // It is much more performant to use HashMaps here instead of CHashMaps
            // because then locking on each comment lookup is not necessary.
            leading: leading.into_iter().collect(),
            trailing: trailing.into_iter().collect(),
            token_finder: TokenFinder::new(tokens.clone(), file_bytes.clone()),
            file_bytes,
            tokens,
            token_index: 0,
        }
    }

    /// Gets the leading comments and all previously unhandled comments.
    pub fn leading_comments_with_previous(&mut self, pos: BytePos) -> Vec<Comment> {
        let mut result = Vec::new();

        if self.token_index == 0 {
            // get any comments stored at the beginning of the file
            // todo: investigate what's required here
            self.append_leading(&mut result, &BytePos(0));
            self.append_trailing(&mut result, &BytePos(0));
        } else if let Some(previous_token) = self.tokens.get(self.token_index - 1) {
            self.append_trailing(&mut result, &previous_token.hi());
        }

        loop {
            if let Some(token) = self.tokens.get(self.token_index) {
                self.append_leading(&mut result, &token.lo());

                let token_hi = token.hi();
                if token_hi < pos {
                    self.append_trailing(&mut result, &token_hi);
                    self.token_index += 1;
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        return result;
    }

    /// Gets the trailing comments and all previously unhandled comments
    pub fn trailing_comments_with_previous(&mut self, end: BytePos) -> Vec<Comment> {
        let mut result = Vec::new();

        loop {
            if let Some(token) = self.tokens.get(self.token_index) {
                self.append_leading(&mut result, &token.lo());

                let is_comma = token.token == Token::Comma;
                if !is_comma && token.lo() >= end {
                    break;
                }

                let token_hi = token.hi();
                if is_comma || token_hi <= end {
                    self.append_trailing(&mut result, &token_hi);
                    self.token_index += 1;
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        // get any comments stored at the end of the file
        if self.token_index >= self.tokens.len() {
            let file_length = self.file_bytes.len() as u32;
            self.append_leading(&mut result, &BytePos(file_length));
        }

        return result;
    }

    pub fn leading_comments(&mut self, pos: BytePos) -> Vec<Comment> {
        let mut result: Vec<Comment> = Vec::new();
        let previous_token_end = self.token_finder.get_previous_token_end_before(&pos);
        self.append_trailing(&mut result, &previous_token_end);
        self.append_leading(&mut result, &pos);
        return result;
    }

    pub fn trailing_comments(&mut self, end: BytePos) -> Vec<Comment> {
        let mut result = Vec::new();
        self.append_trailing(&mut result, &end);
        let end_pos = self.token_finder.get_next_token_pos_after(&end);
        self.append_leading(&mut result, &end_pos);
        return result;
    }

    fn append_trailing(&self, result: &mut Vec<Comment>, pos: &BytePos) {
        if let Some(comments) = self.trailing.get(&pos) {
            result.extend(comments.iter().map(|c| c.clone()));
        }
    }

    fn append_leading(&self, result: &mut Vec<Comment>, pos: &BytePos) {
        if let Some(comments) = self.leading.get(&pos) {
            result.extend(comments.iter().map(|c| c.clone()));
        }
    }
}