use std::sync::Arc;

/// A double ended queue.
pub struct Deque<TItem> {
  reference: Arc<Vec<TItem>>,
  start_index: usize,
  end_index: usize,
}

impl<TItem> Deque<TItem> {
  pub fn new(items: Vec<TItem>) -> Self {
    let end_index = items.len();
    Deque {
      reference: Arc::new(items),
      start_index: 0,
      end_index,
    }
  }

  pub fn len(&self) -> usize {
    self.end_index - self.start_index
  }

  pub fn dequeue<'a>(&'a mut self) -> Option<&'a TItem> {
    if self.start_index == self.end_index {
      None
    } else {
      self.start_index += 1;
      Some(&self.reference[self.start_index - 1])
    }
  }

  pub fn split(&mut self) -> Deque<TItem> {
    let middle_index = self.start_index + self.len() / 2;
    let new_deque = Deque {
      reference: self.reference.clone(),
      start_index: middle_index,
      end_index: self.end_index,
    };

    self.end_index = middle_index;

    new_deque
  }
}
