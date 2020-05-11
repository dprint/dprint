use std::collections::HashSet;
use jsonc_parser::ast::*;
use jsonc_parser::common::{Ranged};
use jsonc_parser::CommentMap;
use super::super::configuration::Configuration;
use super::token_finder::TokenFinder;

pub struct Context<'a> {
    pub config: &'a Configuration,
    pub text: &'a str,
    pub handled_comments: HashSet<usize>,
    pub parent_stack: Vec<Node<'a>>,
    pub current_node: Option<Node<'a>>,
    pub comments: &'a CommentMap,
    pub token_finder: TokenFinder<'a>,
}

impl<'a> Context<'a> {
    pub fn has_handled_comment(&self, comment: &Comment) -> bool {
        self.handled_comments.contains(&comment.start())
    }

    pub fn mark_comment_handled(&mut self, comment: &Comment) {
        self.handled_comments.insert(comment.start());
    }

    pub fn start_line_with_comments(&mut self, node: &dyn Ranged) -> usize {
        // The start position with comments is the next non-whitespace position
        // after the previous token's trailing comments. The trailing comments
        // are similar to the Roslyn definition where it's any comments on the
        // same line or a single multi-line block comment that begins on the trailing line.
        let start = node.start();
        if let Some(leading_comments) = self.comments.get(&start) {
            if let Some(previous_token) = self.token_finder.get_previous_token(node) {
                let previous_end_line = previous_token.end_line();
                let mut past_trailing_comments = false;
                for comment in leading_comments.iter() {
                    let comment_start_line = comment.start_line();
                    if !past_trailing_comments && comment_start_line <= previous_end_line {
                        let comment_end_line = comment.end_line();
                        if comment_end_line > previous_end_line {
                            past_trailing_comments = true;
                        }
                    } else {
                        return comment_start_line;
                    }
                }

                node.start_line()
            } else {
                leading_comments.iter().next().unwrap().start_line()
            }
        } else {
            node.start_line()
        }
    }

    pub fn end_line_with_comments(&mut self, node: &dyn Ranged) -> usize {
        // start searching from after the trailing comma if it exists
        let (search_end, previous_end_line) = self.token_finder
            .get_next_token_if_comma(node).map(|x| (x.end(), x.end_line()))
            .unwrap_or((node.end(), node.end_line()));

        if let Some(trailing_comments) = self.comments.get(&search_end) {
            for comment in trailing_comments.iter() {
                // optimization
                if comment.kind() == CommentKind::Line { break; }

                let comment_start_line = comment.start_line();
                if comment_start_line <= previous_end_line {
                    let comment_end_line = comment.end_line();
                    if comment_end_line > previous_end_line {
                        return comment_end_line; // should only include the first multi-line comment block
                    }
                } else {
                    break;
                }
            }
        }

        previous_end_line
    }

    #[cfg(debug_assertions)]
    pub fn assert_text(&self, start_pos: usize, end_pos: usize, expected_text: &str) {
        let actual_text = &self.text[start_pos..end_pos];
        if actual_text != expected_text {
            panic!("Expected text `{}`, but found `{}`", expected_text, actual_text)
        }
    }
}
