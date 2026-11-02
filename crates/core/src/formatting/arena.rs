use std::sync::Arc;
use std::sync::Mutex;

use super::Condition;
use super::ConditionResolver;
use super::ConditionResolverContext;
use super::PrintNodeCell;
use super::SavePoint;
use super::StringContainer;
use super::WriteItem;
use super::collections::GraphNode;
use super::collections::LookAheadSavePointsNode;
use super::collections::NodeStackNode;
use super::thread_state;
use super::thread_state::Counts;

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
/// The memory of an arena is freed when it's dropped. To reuse it for the next file
/// instead, create the arenas from an [`ArenaPool`].
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
  /// The counts of the infos and conditions created before this arena, which are
  /// restored when it's dropped.
  outer_counts: Counts,
  /// The pool the memory is returned to when the arena is dropped.
  pool: Option<ArenaPool>,
}

impl Default for Arena {
  fn default() -> Self {
    Self::new()
  }
}

impl Drop for Arena {
  fn drop(&mut self) {
    if let Some(pool) = self.pool.take() {
      let mut bump = std::mem::take(&mut self.bump);
      bump.reset();
      pool.lock_bumps().push(bump);
    }
    thread_state::set_counts(std::mem::take(&mut self.outer_counts));
  }
}

impl Arena {
  /// Creates an arena whose memory is freed when it's dropped.
  ///
  /// This also starts the identifiers of the infos and conditions created while it's alive
  /// from zero, which keeps the printer's storage of them small.
  pub fn new() -> Self {
    Self::new_internal(bumpalo::Bump::new(), None)
  }

  fn new_internal(bump: bumpalo::Bump, pool: Option<ArenaPool>) -> Self {
    Self {
      bump,
      outer_counts: thread_state::take_counts(),
      pool,
    }
  }

  /// Gets the bump allocator, which can be used to allocate other values that
  /// should live as long as the print items.
  ///
  /// Note that values allocated in it don't have their destructors run.
  pub fn bump(&self) -> &bumpalo::Bump {
    &self.bump
  }

  /// Allocates a condition resolver in the arena.
  ///
  /// The resolver needs to be `Copy` because the arena doesn't run the destructors of
  /// what's allocated in it. For a resolver that needs state, allocate the state in the
  /// arena and capture a reference to it (ex. `arena.bump().alloc(Cell::new(false))`),
  /// using bumpalo's collections for state that allocates.
  ///
  /// ```compile_fail
  /// use dprint_core::formatting::Arena;
  ///
  /// let arena = Arena::new();
  /// let text = String::from("text");
  /// // a `String` isn't `Copy`, so this would never be dropped
  /// arena.alloc_resolver(move |_| Some(text.is_empty()));
  /// ```
  pub fn alloc_resolver<'a>(&'a self, resolver: impl Fn(&mut ConditionResolverContext) -> Option<bool> + Copy + 'a) -> ConditionResolver<'a> {
    self.bump.alloc(resolver)
  }

  pub(super) fn alloc_condition<'a>(&'a self, condition: Condition<'a>) -> &'a Condition<'a> {
    self.bump.alloc(condition)
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

  pub(super) fn alloc_look_ahead_save_points_node<'a>(&'a self, node: LookAheadSavePointsNode<'a>) -> &'a LookAheadSavePointsNode<'a> {
    self.bump.alloc(node)
  }
}

/// Keeps the memory of dropped arenas so the arenas created after them can reuse it,
/// instead of allocating it again for each file that's formatted.
///
/// The memory is kept until it's cleared or the pool is dropped, so store the pool
/// for as long as it's useful to reuse the memory (ex. in a plugin's state). Each
/// arena's memory keeps the largest chunk it needed, so the pool holds about as much
/// as the largest files that were formatted at the same time.
///
/// ```ignore
/// let arena = self.arena_pool.create_arena();
/// let print_items = generate(&arena, &file);
/// let text = print(print_items, options);
/// ```
///
/// Wasm plugins are given a pool in the format request (`request.arena_pool`).
///
/// Cloning the pool is cheap and the clones share the same memory.
#[derive(Clone, Default)]
pub struct ArenaPool {
  bumps: Arc<Mutex<Vec<bumpalo::Bump>>>,
}

impl ArenaPool {
  pub fn new() -> Self {
    Self::default()
  }

  /// Creates an arena that reuses memory from the pool when there is some, and
  /// returns its memory to the pool when it's dropped.
  pub fn create_arena(&self) -> Arena {
    let bump = self.lock_bumps().pop().unwrap_or_default();
    Arena::new_internal(bump, Some(self.clone()))
  }

  /// Frees the memory kept by the pool.
  ///
  /// The memory of arenas from the pool that are still alive is kept when they're dropped.
  pub fn clear(&self) {
    self.lock_bumps().clear();
  }

  fn lock_bumps(&self) -> std::sync::MutexGuard<'_, Vec<bumpalo::Bump>> {
    // the bumps are still usable if a thread panicked while holding the lock
    self.bumps.lock().unwrap_or_else(|err| err.into_inner())
  }
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn should_reuse_the_memory_of_arenas_from_the_pool() {
    let pool = ArenaPool::new();
    {
      let arena = pool.create_arena();
      arena.bump().alloc_slice_fill_copy(10_000, 0u8);
    }
    assert_eq!(pool.lock_bumps().len(), 1);
    {
      let arena = pool.create_arena();
      assert_eq!(pool.lock_bumps().len(), 0);
      assert!(arena.bump().allocated_bytes() >= 10_000);
    }
    assert_eq!(pool.lock_bumps().len(), 1);
  }

  #[test]
  fn should_keep_the_memory_of_arenas_alive_at_the_same_time() {
    let pool = ArenaPool::new();
    {
      let _outer = pool.create_arena();
      let _inner = pool.create_arena();
    }
    assert_eq!(pool.lock_bumps().len(), 2);
  }

  #[test]
  fn should_free_the_memory_when_cleared() {
    let pool = ArenaPool::new();
    drop(pool.create_arena());
    assert_eq!(pool.lock_bumps().len(), 1);
    pool.clear();
    assert_eq!(pool.lock_bumps().len(), 0);
  }

  #[test]
  fn should_not_keep_the_memory_of_arenas_without_a_pool() {
    let pool = ArenaPool::new();
    drop(Arena::new());
    assert_eq!(pool.lock_bumps().len(), 0);
  }

  #[test]
  fn should_be_able_to_share_the_pool_between_threads() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ArenaPool>();
  }
}
