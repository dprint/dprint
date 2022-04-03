use std::cell::UnsafeCell;

use bumpalo::Bump;

#[derive(Default)]
pub struct Counts {
  info_id_count: u32,
  condition_id_count: u32,
  #[cfg(feature = "tracing")]
  print_node_id_count: u32,
  #[cfg(feature = "tracing")]
  graph_node_id_count: u32,
}

thread_local! {
  static BUMP_ALLOCATOR: UnsafeCell<Bump> = UnsafeCell::new(Bump::new());
  static COUNTS: UnsafeCell<Counts> = UnsafeCell::new(Default::default());
}

pub fn with_bump_allocator<TReturn>(action: impl FnOnce(&Bump) -> TReturn) -> TReturn {
  BUMP_ALLOCATOR.with(|bump_cell| unsafe {
    let bump = bump_cell.get();
    action(&*bump)
  })
}

pub fn with_bump_allocator_mut<TReturn>(action: impl FnMut(&mut Bump) -> TReturn) -> TReturn {
  let mut action = action;
  BUMP_ALLOCATOR.with(|bump_cell| unsafe {
    let bump = bump_cell.get();
    action(&mut *bump)
  })
}

pub fn take_counts() -> Counts {
  COUNTS.with(|cell| unsafe { std::mem::take(&mut (*cell.get())) })
}

pub fn set_counts(counts: Counts) {
  COUNTS.with(|cell| unsafe {
    *cell.get() = counts;
  })
}

pub fn next_info_id() -> u32 {
  COUNTS.with(|cell| unsafe {
    let counts = &mut *cell.get();
    let value = counts.info_id_count;
    counts.info_id_count += 1;
    value
  })
}

pub fn peek_next_info_id() -> u32 {
  COUNTS.with(|cell| unsafe { (*cell.get()).info_id_count })
}

pub fn next_condition_id() -> u32 {
  COUNTS.with(|cell| unsafe {
    let counts = &mut *cell.get();
    let value = counts.condition_id_count;
    counts.condition_id_count += 1;
    value
  })
}

#[cfg(feature = "tracing")]
pub fn next_print_node_id() -> u32 {
  COUNTS.with(|cell| unsafe {
    let counts = &mut *cell.get();
    let value = counts.print_node_id_count;
    counts.print_node_id_count += 1;
    value
  })
}

#[cfg(feature = "tracing")]
pub fn next_graph_node_id() -> u32 {
  COUNTS.with(|cell| unsafe {
    let counts = &mut *cell.get();
    let value = counts.graph_node_id_count;
    counts.graph_node_id_count += 1;
    value
  })
}
