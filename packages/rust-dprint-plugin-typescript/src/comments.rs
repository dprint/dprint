use std::collections::HashMap;
use super::*;
use swc_common::{BytePos, comments::{Comment}};
use swc_ecma_parser::{token::{Token, TokenAndSpan}};

pub struct CommentCollection<'a> {
    leading: &'a HashMap<BytePos, Vec<Comment>>,
    trailing: &'a HashMap<BytePos, Vec<Comment>>,
    token_finder: TokenFinder<'a>,
    file_bytes: &'a Vec<u8>,
    tokens: &'a Vec<TokenAndSpan>,
    token_index: usize,
}

impl<'a> CommentCollection<'a> {
    pub fn new(
        leading: &'a HashMap<BytePos, Vec<Comment>>,
        trailing: &'a HashMap<BytePos, Vec<Comment>>,
        tokens: &'a Vec<TokenAndSpan>,
        file_bytes: &'a Vec<u8>
    ) -> CommentCollection<'a> {
        // println!("Leading: {:?}", leading);
        // println!("Trailing: {:?}", trailing);
        CommentCollection {
            leading: leading,
            trailing: trailing,
            token_finder: TokenFinder::new(tokens, file_bytes),
            file_bytes,
            tokens,
            token_index: 0,
        }
    }

    /// Gets the leading comments and all previously unhandled comments.
    pub fn leading_comments_with_previous(&mut self, pos: BytePos) -> Vec<&'a Comment> {
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
    pub fn trailing_comments_with_previous(&mut self, end: BytePos) -> Vec<&'a Comment> {
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

    pub fn leading_comments(&mut self, pos: BytePos) -> CommentsIterator<'a> {
        let previous_token_end = self.token_finder.get_previous_token_end_before(&pos);
        return CommentsIterator::new(
            self.leading.get(&previous_token_end),
            self.trailing.get(&pos)
        );
    }

    pub fn trailing_comments(&mut self, end: BytePos) -> CommentsIterator<'a> {
        let end_pos = self.token_finder.get_next_token_pos_after(&end);
        return CommentsIterator::new(
            self.trailing.get(&end),
            self.leading.get(&end_pos)
        );
    }

    fn append_trailing(&self, result: &mut Vec<&'a Comment>, pos: &BytePos) {
        if let Some(comments) = self.trailing.get(&pos) {
            result.extend(comments.iter());
        }
    }

    fn append_leading(&self, result: &mut Vec<&'a Comment>, pos: &BytePos) {
        if let Some(comments) = self.leading.get(&pos) {
            result.extend(comments.iter());
        }
    }
}

pub struct CommentsIterator<'a> {
    first: Option<&'a Vec<Comment>>,
    second: Option<&'a Vec<Comment>>,
    first_index: usize,
    second_index: usize,
}

impl<'a> CommentsIterator<'a> {
    pub fn new(first: Option<&'a Vec<Comment>>, second: Option<&'a Vec<Comment>>) -> CommentsIterator<'a> {
        CommentsIterator {
            first,
            second,
            first_index: 0,
            second_index: 0,
        }
    }
}

impl<'a> Iterator for CommentsIterator<'a> {
    type Item = &'a Comment;

    fn next(&mut self) -> Option<&'a Comment> {
        if let Some(first) = self.first {
            if let Some(first_comment) = first.get(self.first_index) {
                self.first_index += 1;
                return Some(first_comment);
            }
        }
        if let Some(second) = self.second {
            if let Some(second_comment) = second.get(self.second_index) {
                self.second_index += 1;
                return Some(second_comment);
            }
        }

        return None;
    }
}

