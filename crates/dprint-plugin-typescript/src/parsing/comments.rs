use std::collections::HashMap;
use super::*;
use swc_common::{BytePos, comments::{Comment}};
use swc_ecma_parser::{token::{Token, TokenAndSpan}};

pub struct CommentCollection<'a> {
    leading: &'a HashMap<BytePos, Vec<Comment>>,
    trailing: &'a HashMap<BytePos, Vec<Comment>>,
    token_finder: TokenFinder<'a>,
    file_bytes: &'a [u8],
    tokens: &'a Vec<TokenAndSpan>,
    token_index: usize,
}

impl<'a> CommentCollection<'a> {
    pub fn new(
        leading: &'a HashMap<BytePos, Vec<Comment>>,
        trailing: &'a HashMap<BytePos, Vec<Comment>>,
        tokens: &'a Vec<TokenAndSpan>,
        file_bytes: &'a [u8],
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
    pub fn leading_comments_with_previous(&mut self, pos: BytePos) -> CommentsIterator<'a> {
        let mut comment_vecs = Vec::new();

        if self.token_index == 0 {
            // get any comments stored at the beginning of the file
            // todo: investigate what's required here
            self.append_leading(&mut comment_vecs, &BytePos(0));
            self.append_trailing(&mut comment_vecs, &BytePos(0));
        } else if let Some(previous_token) = self.tokens.get(self.token_index - 1) {
            self.append_trailing(&mut comment_vecs, &previous_token.hi());
        }

        loop {
            if let Some(token) = self.tokens.get(self.token_index) {
                self.append_leading(&mut comment_vecs, &token.lo());

                let token_hi = token.hi();
                if token_hi < pos {
                    self.append_trailing(&mut comment_vecs, &token_hi);
                    self.token_index += 1;
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        return CommentsIterator::new(comment_vecs);
    }

    /// Gets the trailing comments and all previously unhandled comments
    pub fn trailing_comments_with_previous(&mut self, end: BytePos) -> CommentsIterator<'a> {
        let mut comment_vecs = Vec::new();

        loop {
            if let Some(token) = self.tokens.get(self.token_index) {
                self.append_leading(&mut comment_vecs, &token.lo());

                let is_comma = token.token == Token::Comma;
                if !is_comma && token.lo() >= end {
                    break;
                }

                let token_hi = token.hi();
                if is_comma || token_hi <= end {
                    self.append_trailing(&mut comment_vecs, &token_hi);
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
            self.append_leading(&mut comment_vecs, &BytePos(file_length));
        }

        return CommentsIterator::new(comment_vecs);
    }

    pub fn leading_comments(&mut self, pos: BytePos) -> CommentsIterator<'a> {
        let previous_token_end = self.token_finder.get_previous_token_end_before(&pos);
        let mut comment_vecs = Vec::new();
        self.append_trailing(&mut comment_vecs, &previous_token_end);
        self.append_leading(&mut comment_vecs, &pos);
        return CommentsIterator::new(comment_vecs);
    }

    pub fn trailing_comments(&mut self, end: BytePos) -> CommentsIterator<'a> {
        let end_pos = self.token_finder.get_next_token_pos_after(&end);
        let mut comment_vecs = Vec::new();
        self.append_trailing(&mut comment_vecs, &end);
        self.append_leading(&mut comment_vecs, &end_pos);
        return CommentsIterator::new(comment_vecs);
    }

    fn append_trailing(&self, comment_vecs: &mut Vec<&'a Vec<Comment>>, pos: &BytePos) {
        if let Some(comments) = self.trailing.get(&pos) {
            comment_vecs.push(comments);
        }
    }

    fn append_leading(&self, comment_vecs: &mut Vec<&'a Vec<Comment>>, pos: &BytePos) {
        if let Some(comments) = self.leading.get(&pos) {
            comment_vecs.push(comments);
        }
    }
}

#[derive(Clone)]
pub struct CommentsIterator<'a> {
    comment_vecs: Vec<&'a Vec<Comment>>,
    outer_index: usize,
    inner_index: usize,
}

impl<'a> CommentsIterator<'a> {
    pub fn new(comment_vecs: Vec<&'a Vec<Comment>>) -> CommentsIterator<'a> {
        CommentsIterator {
            comment_vecs,
            outer_index: 0,
            inner_index: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        for comments in self.comment_vecs.iter() {
            if !comments.is_empty() {
                return false;
            }
        }

        true
    }

    pub fn get_last_comment(&self) -> Option<&'a Comment> {
        if let Some(comments) = self.comment_vecs.last() {
            comments.last()
        } else {
            None
        }
    }

    pub fn has_unhandled_comment(&self, context: &mut Context) -> bool {
        for comments in self.comment_vecs.iter() {
            for comment in comments.iter() {
                if !context.has_handled_comment(comment) {
                    return true;
                }
            }
        }

        false
    }
}

impl<'a> Iterator for CommentsIterator<'a> {
    type Item = &'a Comment;

    fn next(&mut self) -> Option<&'a Comment> {
        loop {
            if let Some(comments) = self.comment_vecs.get(self.outer_index) {
                if let Some(comment) = comments.get(self.inner_index) {
                    self.inner_index += 1;
                    return Some(comment);
                } else {
                    self.inner_index = 0;
                    self.outer_index += 1;
                }
            } else {
                return None;
            }
        }
    }
}

