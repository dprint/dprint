pub struct Stack<T> {
    items: Vec<T>,
}

impl<T> Stack<T> {
    pub fn new() -> Stack<T> {
        Stack { items: Vec::new() }
    }

    pub fn pop(&mut self) -> T {
        let result = self.items.pop();
        result.expect("Tried to pop, but the stack was empty. This indicates a bug where an item is being popped, but not pushed to the stack.")
    }

    pub fn push(&mut self, item: T) {
        self.items.push(item);
    }

    pub fn peek(&self) -> Option<&T> {
        self.items.last()
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.items.iter().rev()
    }
}
