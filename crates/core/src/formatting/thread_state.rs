use std::cell::UnsafeCell;

use bumpalo::Bump;

#[derive(Default)]
pub struct Counts {
  line_number_anchor_id_count: u32,
  line_number_id_count: u32,
  column_number_id_count: u32,
  is_start_of_line_id: u32,
  indent_level_id_count: u32,
  line_start_column_number_id_count: u32,
  line_start_indent_level_id_count: u32,
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

pub fn next_line_number_anchor_id() -> u32 {
  COUNTS.with(|cell| unsafe {
    let counts = &mut *cell.get();
    let value = counts.line_number_anchor_id_count;
    counts.line_number_anchor_id_count += 1;
    value
  })
}

pub fn next_line_number_id() -> u32 {
  COUNTS.with(|cell| unsafe {
    let counts = &mut *cell.get();
    let value = counts.line_number_id_count;
    counts.line_number_id_count += 1;
    value
  })
}

pub fn next_column_number_id() -> u32 {
  COUNTS.with(|cell| unsafe {
    let counts = &mut *cell.get();
    let value = counts.column_number_id_count;
    counts.column_number_id_count += 1;
    value
  })
}

pub fn next_is_start_of_line_id() -> u32 {
  COUNTS.with(|cell| unsafe {
    let counts = &mut *cell.get();
    let value = counts.is_start_of_line_id;
    counts.is_start_of_line_id += 1;
    value
  })
}

pub fn next_indent_level_id() -> u32 {
  COUNTS.with(|cell| unsafe {
    let counts = &mut *cell.get();
    let value = counts.indent_level_id_count;
    counts.indent_level_id_count += 1;
    value
  })
}

pub fn next_line_start_column_number_id() -> u32 {
  COUNTS.with(|cell| unsafe {
    let counts = &mut *cell.get();
    let value = counts.line_start_column_number_id_count;
    counts.line_start_column_number_id_count += 1;
    value
  })
}

pub fn next_line_start_indent_level_id() -> u32 {
  COUNTS.with(|cell| unsafe {
    let counts = &mut *cell.get();
    let value = counts.line_start_indent_level_id_count;
    counts.line_start_indent_level_id_count += 1;
    value
  })
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
