use std::cell::RefCell;

use super::Condition;
use super::ConditionResolver;
use super::PrintNodeCell;
use super::SavePoint;
use super::StringContainer;
use super::WriteItem;
use super::collections::GraphNode;
use super::collections::NodeStackNode;
use super::thread_state;
use super::thread_state::Counts;

thread_local! {
  /// The memory of dropped arenas, which is kept so it can be reused by later arenas.
  static BUMP_POOL: RefCell<Vec<bumpalo::Bump>> = const { RefCell::new(Vec::new()) };
}

/// The memory that print items, and the printer's state when printing them, are allocated in.
///
/// Everything allocated in an arena lives as long as the arena is borrowed, so the
/// compiler ensures print items aren't used after the arena is dropped. Since the
/// arena is created by the caller, the print items can also borrow data that outlives
/// it, such as the text of the file being formatted.
///
/// ```ignore
/// let arena = Arena::new();
/// let print_items = generate(&arena, &file);
/// let text = print(print_items, options);
/// ```
///
/// An arena should be created for each file that's formatted, and arenas should be
/// dropped in the reverse order they're created in (ex. when formatting within a format).
///
/// Print items can't be used after their arena is dropped:
///
/// ```compile_fail
/// use dprint_core::formatting::Arena;
/// use dprint_core::formatting::PrintItems;
///
/// let print_items = {
///   let arena = Arena::new();
///   PrintItems::new(&arena)
/// };
/// ```
pub struct Arena {
  bump: bumpalo::Bump,
  /// The resolvers of the conditions allocated in the arena.
  ///
  /// The arena doesn't run the destructors of what's allocated in it, so this owns
  /// the reference counts of the resolvers that would otherwise never be released.
  condition_resolvers: RefCell<Vec<ConditionResolver>>,
  /// The counts of the infos and conditions created before this arena, which are
  /// restored when it's dropped.
  outer_counts: Counts,
}

impl Default for Arena {
  fn default() -> Self {
    Self::new()
  }
}

impl Drop for Arena {
  fn drop(&mut self) {
    self.condition_resolvers.get_mut().clear();
    let mut bump = std::mem::take(&mut self.bump);
    bump.reset();
    BUMP_POOL.with(|pool| pool.borrow_mut().push(bump));
    thread_state::set_counts(std::mem::take(&mut self.outer_counts));
  }
}

impl Arena {
  /// Creates an arena, reusing the memory of a dropped arena on this thread when there is one.
  ///
  /// This also starts the identifiers of the infos and conditions created while it's alive
  /// from zero, which keeps the printer's storage of them small.
  pub fn new() -> Self {
    Self {
      bump: BUMP_POOL.with(|pool| pool.borrow_mut().pop()).unwrap_or_default(),
      condition_resolvers: Default::default(),
      outer_counts: thread_state::take_counts(),
    }
  }

  /// Gets the bump allocator, which can be used to allocate other values that
  /// should live as long as the print items.
  ///
  /// Note that values allocated in it don't have their destructors run.
  pub fn bump(&self) -> &bumpalo::Bump {
    &self.bump
  }

  pub(super) fn alloc_condition<'a>(&'a self, condition: Condition<'a>) -> &'a Condition<'a> {
    let condition = self.bump.alloc(condition);
    // SAFETY: The condition in the bump allocator is never dropped, so its resolver's
    // reference count is moved to this list, which releases it when the arena is
    // reset or dropped. That only happens once nothing borrows the arena, so nothing
    // can use the resolver in the condition afterwards.
    let resolver = unsafe { std::ptr::read(&condition.condition) };
    self.condition_resolvers.borrow_mut().push(resolver);
    condition
  }

  /// Copies the text into the arena.
  pub(super) fn alloc_str<'a>(&'a self, text: &str) -> &'a StringContainer<'a> {
    let text = self.bump.alloc_str(text);
    self.alloc_string_container(text)
  }

  pub(super) fn alloc_string_container<'a>(&'a self, text: &'a str) -> &'a StringContainer<'a> {
    self.bump.alloc(StringContainer::new(text))
  }

  pub(super) fn alloc_print_node_cell<'a>(&'a self, cell: PrintNodeCell<'a>) -> &'a PrintNodeCell<'a> {
    self.bump.alloc(cell)
  }

  pub(super) fn alloc_write_item_graph_node<'a>(&'a self, node: GraphNode<'a, WriteItem<'a>>) -> &'a GraphNode<'a, WriteItem<'a>> {
    self.bump.alloc(node)
  }

  pub(super) fn alloc_node_stack_node<'a>(&'a self, node: NodeStackNode<'a>) -> &'a NodeStackNode<'a> {
    self.bump.alloc(node)
  }

  pub(super) fn alloc_save_point<'a>(&'a self, save_point: SavePoint<'a>) -> &'a SavePoint<'a> {
    self.bump.alloc(save_point)
  }
}
