use std::borrow::Cow;
use std::cell::UnsafeCell;
use std::rc::Rc;

use super::collections::GraphNode;
use super::collections::NodeStackNode;
use super::Condition;
use super::ConditionResolver;
use super::PrintNodeCell;
use super::SavePoint;
use super::StringContainer;
use super::UnsafePrintLifetime;
use super::WriteItem;

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
  condition_reevaluation_id_count: u32,
  #[cfg(feature = "tracing")]
  print_node_id_count: u32,
  #[cfg(feature = "tracing")]
  graph_node_id_count: u32,
}

/// This file is a dumpster fireand the API is actually really
/// unsafe if used incorrectly. The way it ended up this way was
/// because I wanted to try out the performance improvements of
/// a bump allocator without having to deal with lifetimes everywhere.
/// Anyway, this landed and I haven't had time to go through and
/// make the API safe.
pub struct BumpAllocator {
  condition_resolvers: Vec<ConditionResolver>,
  bump: bumpalo::Bump,
}

impl BumpAllocator {
  fn new() -> Self {
    Self {
      condition_resolvers: Default::default(),
      bump: bumpalo::Bump::new(),
    }
  }

  pub fn inner(&self) -> &bumpalo::Bump {
    &self.bump
  }

  pub fn alloc_condition(&mut self, condition: Condition) -> UnsafePrintLifetime<Condition> {
    unsafe {
      // Leak the Rc that gets stored in the bump allocator, then add another
      // rc in a vector that we store here, which will cause a decrement of the
      // rc when this is dropped.
      let condition_resolver = condition.condition.clone();
      let rc_raw = Rc::into_raw(condition_resolver);
      Rc::decrement_strong_count(rc_raw);
      self.condition_resolvers.push(Rc::from_raw(rc_raw));
    }
    let condition = self.bump.alloc(condition);
    unsafe { std::mem::transmute::<&Condition, UnsafePrintLifetime<Condition>>(condition) }
  }

  pub fn alloc_string(&self, item: Cow<'static, str>) -> UnsafePrintLifetime<StringContainer> {
    let string = match item {
      Cow::Borrowed(item) => item,
      Cow::Owned(item) => {
        let string = self.bump.alloc(bumpalo::collections::String::from_str_in(&item, &self.bump));
        unsafe { std::mem::transmute::<&bumpalo::collections::String, UnsafePrintLifetime<bumpalo::collections::String>>(string) }
      }
    };
    let string = StringContainer::new(string);
    let string = self.bump.alloc(string);
    unsafe { std::mem::transmute::<&StringContainer, UnsafePrintLifetime<StringContainer>>(string) }
  }

  pub fn alloc_write_item_graph_node<'a>(&'a self, node: GraphNode<'a, WriteItem<'a>>) -> &'a GraphNode<'a, WriteItem<'a>> {
    self.bump.alloc(node)
  }

  pub fn alloc_node_stack_node<'a>(&'a self, node: NodeStackNode<'a>) -> &'a NodeStackNode<'a> {
    self.bump.alloc(node)
  }

  pub fn alloc_save_point<'a>(&'a self, save_point: SavePoint<'a>) -> &'a SavePoint {
    self.bump.alloc(save_point)
  }

  pub fn alloc_print_node_cell(&self, cell: PrintNodeCell) -> UnsafePrintLifetime<PrintNodeCell> {
    let result = self.bump.alloc(cell);
    unsafe { std::mem::transmute::<&PrintNodeCell, UnsafePrintLifetime<PrintNodeCell>>(result) }
  }

  pub fn reset(&mut self) {
    self.bump.reset();
    self.condition_resolvers.clear();
  }
}

thread_local! {
  static BUMP_ALLOCATOR: UnsafeCell<BumpAllocator> = UnsafeCell::new(BumpAllocator::new());
  static COUNTS: UnsafeCell<Counts> = UnsafeCell::new(Default::default());
}

pub fn with_bump_allocator<TReturn>(action: impl FnOnce(&mut BumpAllocator) -> TReturn) -> TReturn {
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

pub fn next_condition_reevaluation_id() -> u32 {
  COUNTS.with(|cell| unsafe {
    let counts = &mut *cell.get();
    let value = counts.condition_reevaluation_id_count;
    counts.condition_reevaluation_id_count += 1;
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
