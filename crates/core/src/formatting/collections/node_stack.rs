use crate::formatting::Arena;
use crate::formatting::PrintItemPath;

#[derive(Clone)]
pub struct NodeStackNode<'a> {
  path: PrintItemPath<'a>,
  next: Option<&'a NodeStackNode<'a>>,
}

#[derive(Default, Clone)]
pub struct NodeStack<'a>(Option<&'a NodeStackNode<'a>>);

impl<'a> NodeStack<'a> {
  pub fn push(&mut self, path: PrintItemPath<'a>, arena: &'a Arena) {
    let next = self.0;
    self.0 = Some(arena.alloc_node_stack_node(NodeStackNode { path, next }));
  }

  pub fn pop(&mut self) -> Option<PrintItemPath<'a>> {
    let head = self.0?;
    self.0 = head.next;
    Some(head.path)
  }

  pub fn is_empty(&self) -> bool {
    self.0.is_none()
  }
}
