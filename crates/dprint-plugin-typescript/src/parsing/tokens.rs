use std::str;
use super::*;
use swc_common::{BytePos, SpanData};
use swc_ecma_parser::{token::{Token, TokenAndSpan}};
use dprint_core::tokens::{TokenFinder as CoreTokenFinder, TokenCollection};

pub struct TokenFinder<'a> {
    inner: CoreTokenFinder<LocalTokenCollection<'a>>,
    file_bytes: &'a [u8],
}

impl<'a> TokenFinder<'a> {
    pub fn new(tokens: &'a Vec<TokenAndSpan>, file_bytes: &'a [u8]) -> TokenFinder<'a> {
        TokenFinder {
            inner: CoreTokenFinder::new(LocalTokenCollection(tokens)),
            file_bytes,
        }
    }

    pub fn get_char_at(&self, pos: &BytePos) -> char {
        self.file_bytes[pos.0 as usize] as char
    }

    pub fn get_previous_token_if_open_paren(&mut self, node: &dyn Ranged) -> Option<&'a TokenAndSpan> {
        self.get_previous_token_if(node, |token| token.token == Token::LParen)
    }

    pub fn get_next_token_if_close_paren(&mut self, node: &dyn Ranged) -> Option<&'a TokenAndSpan> {
        self.get_next_token_if(node, |token| token.token == Token::RParen)
    }

    pub fn get_previous_token_if_open_brace(&mut self, node: &dyn Ranged) -> Option<&'a TokenAndSpan> {
        self.get_previous_token_if(node, |token| token.token == Token::LBrace)
    }

    pub fn get_previous_token_if_open_bracket(&mut self, node: &dyn Ranged) -> Option<&'a TokenAndSpan> {
        self.get_previous_token_if(node, |token| token.token == Token::LBracket)
    }

    pub fn get_previous_token_if_close_brace(&mut self, node: &dyn Ranged) -> Option<&'a TokenAndSpan> {
        self.get_previous_token_if(node, |token| token.token == Token::RBrace)
    }

    pub fn get_previous_token_if_from_keyword(&mut self, node: &dyn Ranged) -> Option<&'a TokenAndSpan> {
        let file_bytes = self.file_bytes;
        self.get_previous_token_if(node, |token| get_text(file_bytes, &token.span) == "from")
    }

    pub fn get_previous_token_if_colon(&mut self, node: &dyn Ranged) -> Option<&'a TokenAndSpan> {
        self.get_previous_token_if(node, |token| token.token == Token::Colon)
    }

    pub fn get_previous_token_if_operator(&mut self, node: &dyn Ranged, operator_text: &str) -> Option<&'a TokenAndSpan> {
        let file_bytes = self.file_bytes;
        self.get_previous_token_if(node, |token| get_text(file_bytes, &token.span) == operator_text)
    }

    #[inline]
    pub fn get_previous_token(&mut self, node: &dyn Ranged) -> Option<&'a TokenAndSpan> {
        self.inner.get_previous_token(node.lo())
    }

    pub fn get_next_token_if_comma(&mut self, node: &dyn Ranged) -> Option<&'a TokenAndSpan> {
        self.get_next_token_if(node, |token| token.token == Token::Comma)
    }

    pub fn get_next_token_if_close_bracket(&mut self, node: &dyn Ranged) -> Option<&'a TokenAndSpan> {
        self.get_next_token_if(node, |token| token.token == Token::RBracket)
    }

    pub fn get_first_open_brace_token_within(&mut self, node: &dyn Ranged) -> Option<&'a TokenAndSpan> {
        self.get_first_token_within(node, |token| token.token == Token::LBrace)
    }

    pub fn get_last_close_brace_token_within(&mut self, node: &dyn Ranged) -> Option<&'a TokenAndSpan> {
        self.get_last_token_within(node, |token| token.token == Token::RBrace)
    }

    pub fn get_first_open_bracket_token_within(&mut self, node: &dyn Ranged) -> Option<&'a TokenAndSpan> {
        self.get_first_token_within(node, |token| token.token == Token::LBracket)
    }

    pub fn get_first_comma_within(&mut self, node: &dyn Ranged) -> Option<&'a TokenAndSpan> {
        self.get_first_token_within(node, |token| token.token == Token::Comma)
    }

    pub fn get_first_semi_colon_within(&mut self, node: &dyn Ranged) -> Option<&'a TokenAndSpan> {
        self.get_first_token_within(node, |token| token.token == Token::Semi)
    }

    pub fn get_first_semi_colon_after(&mut self, node: &dyn Ranged) -> Option<&'a TokenAndSpan> {
        self.get_first_token_after(node, |token| token.token == Token::Semi)
    }

    pub fn get_first_colon_token_after(&mut self, node: &dyn Ranged) -> Option<&'a TokenAndSpan> {
        self.get_first_token_after(node, |token| token.token == Token::Colon)
    }

    pub fn get_first_colon_token_within(&mut self, node: &dyn Ranged) -> Option<&'a TokenAndSpan> {
        self.get_first_token_within(node, |token| token.token == Token::Colon)
    }

    pub fn get_first_operator_after(&mut self, node: &dyn Ranged, operator_text: &str) -> Option<&'a TokenAndSpan> {
        self.get_first_token_after_with_text(node, operator_text)
    }

    pub fn get_first_keyword_after(&mut self, node: &dyn Ranged, keyword_text: &str) -> Option<&'a TokenAndSpan> {
        self.get_first_token_after_with_text(node, keyword_text)
    }

    pub fn get_first_else_keyword_within(&mut self, node: &dyn Ranged) -> Option<&'a TokenAndSpan> {
        let file_bytes = self.file_bytes;
        self.get_first_token_within(node, |token| get_text(file_bytes, &token.span) == "else")
    }

    pub fn get_first_open_brace_token_before(&mut self, node: &dyn Ranged) -> Option<&'a TokenAndSpan> {
        self.get_first_token_before(node, |token| token.token == Token::LBrace)
    }

    pub fn get_first_open_paren_before(&mut self, node: &dyn Ranged) -> Option<&'a TokenAndSpan> {
        self.get_first_token_before(node, |token| token.token == Token::LParen)
    }

    pub fn get_first_close_paren_before(&mut self, node: &dyn Ranged) -> Option<&'a TokenAndSpan> {
        self.get_first_token_before(node, |token| token.token == Token::RParen)
    }

    pub fn get_first_close_paren_after(&mut self, node: &dyn Ranged) -> Option<&'a TokenAndSpan> {
        self.get_first_token_after(node, |token| token.token == Token::RParen)
    }

    pub fn get_previous_token_end_before(&mut self, node: &dyn Ranged) -> BytePos {
        let previous_token = self.get_previous_token(node);
        if let Some(token) = previous_token {
            token.span.hi()
        } else {
            BytePos(0)
        }
    }

    pub fn get_next_token_pos_after(&mut self, node: &dyn Ranged) -> BytePos {
        let next_token = self.get_next_token(node);
        if let Some(token) = next_token {
            token.span.lo()
        } else {
            BytePos(self.file_bytes.len() as u32)
        }
    }

    #[inline]
    fn get_first_token_after_with_text(&mut self, node: &dyn Ranged, text: &str) -> Option<&'a TokenAndSpan> {
        let file_bytes = self.file_bytes;
        self.get_first_token_after(node, |token| get_text(file_bytes, &token.span) == text)
    }

    #[inline]
    fn get_next_token_if(&mut self, node: &dyn Ranged, is_match: impl FnOnce(&TokenAndSpan) -> bool) -> Option<&'a TokenAndSpan> {
        self.inner.get_next_token_if(node.hi(), is_match)
    }

    #[inline]
    fn get_previous_token_if(&mut self, node: &dyn Ranged, is_match: impl FnOnce(&TokenAndSpan) -> bool) -> Option<&'a TokenAndSpan> {
        self.inner.get_previous_token_if(node.lo(), is_match)
    }

    #[inline]
    fn get_next_token(&mut self, node: &dyn Ranged) -> Option<&'a TokenAndSpan> {
        self.inner.get_next_token(node.hi())
    }

    #[inline]
    fn get_first_token_before(&mut self, node: &dyn Ranged, is_match: impl Fn(&TokenAndSpan) -> bool) -> Option<&'a TokenAndSpan> {
        self.inner.get_first_token_before(node.lo(), is_match)
    }

    #[inline]
    fn get_first_token_after(&mut self, node: &dyn Ranged, is_match: impl Fn(&'a TokenAndSpan) -> bool) -> Option<&'a TokenAndSpan> {
        self.inner.get_first_token_after(node.hi(), is_match)
    }

    #[inline]
    fn get_first_token_within(&mut self, node: &dyn Ranged, is_match: impl Fn(&'a TokenAndSpan) -> bool) -> Option<&'a TokenAndSpan> {
        let node_span_data = node.span_data();
        self.inner.get_first_token_within(node_span_data.lo, node_span_data.hi, is_match)
    }

    #[inline]
    fn get_last_token_within(&mut self, node: &dyn Ranged, is_match: impl Fn(&'a TokenAndSpan) -> bool) -> Option<&'a TokenAndSpan> {
        let node_span_data = node.span_data();
        self.inner.get_last_token_within(node_span_data.lo, node_span_data.hi, is_match)
    }
}

fn get_text<'a>(file_bytes: &'a [u8], span_data: &SpanData) -> &'a str {
    let bytes = &file_bytes[(span_data.lo.0 as usize)..(span_data.hi.0 as usize)];
    str::from_utf8(&bytes).unwrap()
}

// Wrap and implement a trait for the CoreTokenFinder

struct LocalTokenCollection<'a>(&'a Vec<TokenAndSpan>);

impl<'a> TokenCollection<'a> for LocalTokenCollection<'a> {
    type TPos = BytePos;
    type TToken = TokenAndSpan;
    fn get_start_at_index(&self, index: usize) -> BytePos { self.0[index].span.lo }
    fn get_end_at_index(&self, index: usize) -> BytePos { self.0[index].span.hi }
    fn get_token_at_index(&self, index: usize) -> &'a TokenAndSpan { &self.0[index] }
    fn len(&self) -> usize { self.0.len() }
    fn is_empty(&self) -> bool { self.0.is_empty() }
}
