use crate::formatting::PrintItemPath;
use crate::formatting::thread_state::BumpAllocator;

#[derive(Clone)]
pub struct NodeStackNode<'a> {
  path: PrintItemPath,
  next: Option<&'a NodeStackNode<'a>>,
}

#[derive(Default, Clone)]
pub struct NodeStack<'a>(Option<&'a NodeStackNode<'a>>);

impl<'a> NodeStack<'a> {
  pub fn push(&mut self, path: PrintItemPath, bump: &'a BumpAllocator) {
    let next = self.0;
    self.0 = Some(bump.alloc_node_stack_node(NodeStackNode { path, next }));
  }

  pub fn pop(&mut self) -> Option<PrintItemPath> {
    let head = self.0?;
    self.0 = head.next;
    Some(head.path)
  }

  pub fn is_empty(&self) -> bool {
    self.0.is_none()
  }
}
