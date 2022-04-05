use std::rc::Rc;

use crate::formatting::PrintItemPath;

#[derive(Clone)]
struct RcNode {
  path: PrintItemPath,
  next: Option<Rc<RcNode>>,
}

#[derive(Default, Clone)]
pub struct RcStack {
  size: usize,
  node: Option<Rc<RcNode>>,
}

impl RcStack {
  pub fn push(&mut self, path: PrintItemPath) {
    let next = self.node.as_ref().map(Rc::clone);
    self.node = Some(Rc::new(RcNode { path, next }));
    self.size += 1;
  }

  pub fn pop(&mut self) -> Option<PrintItemPath> {
    let head = Rc::clone(self.node.as_ref()?);
    self.node = head.next.as_ref().cloned();
    self.size -= 1;
    Some(head.path)
  }

  pub fn size(&self) -> usize {
    self.size
  }
}
