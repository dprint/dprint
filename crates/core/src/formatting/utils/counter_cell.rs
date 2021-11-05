use std::cell::UnsafeCell;

pub struct CounterCell {
  counter: UnsafeCell<usize>,
}

impl CounterCell {
  pub fn new() -> CounterCell {
    CounterCell {
      counter: UnsafeCell::default(),
    }
  }

  pub fn increment_and_get(&self) -> usize {
    unsafe {
      let count = self.counter.get();
      *count += 1;
      *count
    }
  }
}
