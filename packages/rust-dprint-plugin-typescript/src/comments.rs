use std::rc::Rc;
use super::*;
use std::collections::{HashMap};
use swc_common::{SpanData, BytePos, comments::{Comments, Comment, CommentMap}, Span};
use swc_ecma_parser::{token::{Token, TokenAndSpan}};

pub struct CommentCollection {
    leading: CommentMap,
    trailing: CommentMap,
    token_finder: TokenFinder,
}

impl CommentCollection {
    pub fn new(comments: Comments, token_finder: TokenFinder) -> CommentCollection {
        let (leading, trailing) = comments.take_all();
        println!("Leading: {:?}", leading);
        println!("Trailing: {:?}", trailing);
        CommentCollection {
            leading,
            trailing,
            token_finder,
        }
    }

    pub fn trailing_comments(&mut self, end: BytePos) -> Vec<Comment> {
        let mut result = Vec::new();
        if let Some(comments) = self.trailing.get(&end) {
            result.extend(comments.iter().map(|c| c.clone()));
        }
        if let Some(token_after) = self.token_finder.get_first_token_after(end, |_| true) {
            if let Some(comments) = self.leading.get(&token_after.lo()) {
                result.extend(comments.iter().map(|c| c.clone()));
            }
        }
        return result;
    }

    pub fn leading_comments(&mut self, pos: BytePos) -> Vec<Comment> {
        let mut result: Vec<Comment> = Vec::new();
        if let Some(token_before) = self.token_finder.get_first_token_before(pos, |_| true) {
            if let Some(comments) = self.trailing.get(&token_before.hi()) {
                result.extend(comments.iter().map(|c| c.clone()));
            }
        }
        if let Some(comments) = self.leading.get(&pos) {
            result.extend(comments.iter().map(|c| c.clone()));
        }
        return result;
    }
}