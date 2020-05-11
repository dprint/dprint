/// Trait for a collection of tokens.
pub trait TokenCollection<'a> {
    /// The position type the token uses.
    type TPos: PartialOrd + Copy;
    /// The token type.
    type TToken: 'a;

    /// Gets the start position at the specified collection index.
    fn get_start_at_index(&self, index: usize) -> Self::TPos;
    /// Gets the end position at the specified collection index.
    fn get_end_at_index(&self, index: usize) -> Self::TPos;
    /// Gets the token at the specified collection index.
    fn get_token_at_index(&self, index: usize) -> &'a Self::TToken;
    /// Gets the length of the collection.
    fn len(&self) -> usize;
    /// Gets if the collection is empty.
    fn is_empty(&self) -> bool;
}

/// Searches a token collection for
pub struct TokenFinder<TTokenCollection> {
    tokens: TTokenCollection,
    token_index: usize,
}

impl<'a, TTokenCollection> TokenFinder<TTokenCollection> where TTokenCollection : TokenCollection<'a> {
    pub fn new(tokens: TTokenCollection) -> TokenFinder<TTokenCollection> {
        TokenFinder {
            tokens,
            token_index: 0,
        }
    }

    #[inline]
    pub fn get_next_token_if(
        &mut self,
        end: TTokenCollection::TPos,
        is_match: impl FnOnce(&'a TTokenCollection::TToken) -> bool
    ) -> Option<&'a TTokenCollection::TToken> {
        let next_token = self.get_next_token(end)?;
        return if is_match(next_token) { Some(next_token) } else { None };
    }

    #[inline]
    pub fn get_next_token(&mut self, end: TTokenCollection::TPos) -> Option<&'a TTokenCollection::TToken> {
        self.get_first_token_after(end, |_| true)
    }

    #[inline]
    pub fn get_previous_token_if(
        &mut self,
        start: TTokenCollection::TPos,
        is_match: impl FnOnce(&'a TTokenCollection::TToken) -> bool
    ) -> Option<&'a TTokenCollection::TToken> {
        let previous_token = self.get_previous_token(start)?;
        return if is_match(&previous_token) { Some(previous_token) } else { None };
    }

    #[inline]
    pub fn get_previous_token(&mut self, start: TTokenCollection::TPos) -> Option<&'a TTokenCollection::TToken> {
        self.get_first_token_before(start, |_| true)
    }

    pub fn get_first_token_within(
        &mut self,
        start: TTokenCollection::TPos,
        end: TTokenCollection::TPos,
        is_match: impl Fn(&'a TTokenCollection::TToken) -> bool
    ) -> Option<&'a TTokenCollection::TToken> {
        if self.tokens.is_empty() { return None; }
        self.move_to_node_start(start);

        loop {
            let token_start = self.tokens.get_start_at_index(self.token_index);
            if token_start >= end {
                break;
            } else {
                let current_token = self.tokens.get_token_at_index(self.token_index);
                if is_match(current_token) {
                    return Some(current_token);
                }
            }

            if !self.try_increment_index() {
                break;
            }
        }

        None
    }

    pub fn get_last_token_within(
        &mut self,
        start: TTokenCollection::TPos,
        end: TTokenCollection::TPos,
        is_match: impl Fn(&'a TTokenCollection::TToken) -> bool
    ) -> Option<&'a TTokenCollection::TToken> {
        if self.tokens.is_empty() { return None; }

        self.move_to_node_end(end);

        loop {
            let token_start = self.tokens.get_start_at_index(self.token_index);
            if token_start >= end || token_start < start {
                break;
            } else {
                let current_token = self.tokens.get_token_at_index(self.token_index);
                if is_match(current_token) {
                    return Some(current_token);
                }
            }

            if !self.try_decrement_index() {
                break;
            }
        }

        None
    }

    pub fn get_first_token_before(
        &mut self,
        start: TTokenCollection::TPos,
        is_match: impl Fn(&'a TTokenCollection::TToken) -> bool
    ) -> Option<&'a TTokenCollection::TToken> {
        if self.tokens.is_empty() { return None; }
        self.move_to_node_start(start);

        if self.tokens.get_start_at_index(self.token_index) < start {
            let current_token = self.tokens.get_token_at_index(self.token_index);
            if is_match(current_token) {
                return Some(current_token);
            }
        }

        while self.try_decrement_index() {
            let current_token = self.tokens.get_token_at_index(self.token_index);
            if is_match(current_token) {
                return Some(current_token);
            }
        }

        return None;
    }

    pub fn get_first_token_after(
        &mut self,
        end: TTokenCollection::TPos,
        is_match: impl Fn(&'a TTokenCollection::TToken) -> bool
    ) -> Option<&'a TTokenCollection::TToken> {
        if self.tokens.is_empty() { return None; }
        self.move_to_node_end(end);

        while self.try_increment_index() {
            let current_token = self.tokens.get_token_at_index(self.token_index);
            if is_match(current_token) {
                return Some(current_token);
            }
        }

        None
    }

    fn move_to_node_start(&mut self, start: TTokenCollection::TPos) {
        while self.tokens.get_start_at_index(self.token_index) < start {
            if !self.try_increment_index() {
                break;
            }
        }

        while self.tokens.get_start_at_index(self.token_index) > start {
            if !self.try_decrement_index() {
                break;
            }
        }
    }

    fn move_to_node_end(&mut self, end: TTokenCollection::TPos) {
        while self.tokens.get_end_at_index(self.token_index) < end {
            if !self.try_increment_index() {
                break;
            }
        }

        while self.tokens.get_end_at_index(self.token_index) > end {
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
