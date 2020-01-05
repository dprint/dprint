use super::*;
use swc_common::{BytePos, comments::{Comments, Comment, CommentMap}};

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
        let token_pos = self.token_finder.get_first_token_before(pos, |_| true)
            .map(|t| t.hi())
            .unwrap_or(BytePos(0)); // start of file
        if let Some(comments) = self.trailing.get(&token_pos) {
            result.extend(comments.iter().map(|c| c.clone()));
        }
        if let Some(comments) = self.leading.get(&pos) {
            result.extend(comments.iter().map(|c| c.clone()));
        }
        return result;
    }
}