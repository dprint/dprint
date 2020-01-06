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
        //println!("Leading: {:?}", leading);
        //println!("Trailing: {:?}", trailing);
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
        let end_pos = self.token_finder.get_next_token_pos_after(&end);
        if let Some(comments) = self.leading.get(&end_pos.lo()) {
            result.extend(comments.iter().map(|c| c.clone()));
        }
        return result;
    }

    pub fn leading_comments(&mut self, pos: BytePos) -> Vec<Comment> {
        let mut result: Vec<Comment> = Vec::new();
        let token_pos = self.token_finder.get_previous_token_pos_before(&pos);
        if let Some(comments) = self.trailing.get(&token_pos) {
            result.extend(comments.iter().map(|c| c.clone()));
        }
        if let Some(comments) = self.leading.get(&pos) {
            result.extend(comments.iter().map(|c| c.clone()));
        }
        return result;
    }
}