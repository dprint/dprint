use crate::formatting::Arena;
use crate::formatting::SavePoint;

/// What a condition looked ahead at before it was resolved.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LookAheadKey {
  Condition(u32),
  LineNumber(u32),
  ColumnNumber(u32),
  IsStartOfLine(u32),
  IndentLevel(u32),
  LineStartColumnNumber(u32),
  LineStartIndentLevel(u32),
}

/// The save points to go back to once something a condition looked ahead at is resolved.
///
/// This is an immutable list allocated in the arena, so a save point can keep
/// the current list by copying a pointer. There are usually only a few entries.
#[derive(Default, Clone, Copy)]
pub struct LookAheadSavePoints<'a>(Option<&'a LookAheadSavePointsNode<'a>>);

pub struct LookAheadSavePointsNode<'a> {
  key: LookAheadKey,
  save_point: &'a SavePoint<'a>,
  next: Option<&'a LookAheadSavePointsNode<'a>>,
}

impl<'a> LookAheadSavePoints<'a> {
  pub fn contains(&self, key: LookAheadKey) -> bool {
    self.iter().any(|node| node.key == key)
  }

  /// Adds the save point for a key that isn't in the list.
  pub fn insert(&mut self, key: LookAheadKey, save_point: &'a SavePoint<'a>, arena: &'a Arena) {
    debug_assert!(!self.contains(key));
    self.0 = Some(arena.alloc_look_ahead_save_points_node(LookAheadSavePointsNode { key, save_point, next: self.0 }));
  }

  /// Removes the key, returning its save point.
  pub fn remove(&mut self, key: LookAheadKey, arena: &'a Arena) -> Option<&'a SavePoint<'a>> {
    let (list, save_point) = without_key(self.0?, key, arena)?;
    self.0 = list;
    return Some(save_point);

    /// Gets the list without the key, copying the nodes before it.
    fn without_key<'a>(
      node: &'a LookAheadSavePointsNode<'a>,
      key: LookAheadKey,
      arena: &'a Arena,
    ) -> Option<(Option<&'a LookAheadSavePointsNode<'a>>, &'a SavePoint<'a>)> {
      if node.key == key {
        return Some((node.next, node.save_point));
      }
      let (next, save_point) = without_key(node.next?, key, arena)?;
      let node = arena.alloc_look_ahead_save_points_node(LookAheadSavePointsNode {
        key: node.key,
        save_point: node.save_point,
        next,
      });
      Some((Some(node), save_point))
    }
  }

  pub fn save_points(&self) -> impl Iterator<Item = &'a SavePoint<'a>> {
    self.iter().map(|node| node.save_point)
  }

  fn iter(&self) -> impl Iterator<Item = &'a LookAheadSavePointsNode<'a>> {
    std::iter::successors(self.0, |node| node.next)
  }
}
