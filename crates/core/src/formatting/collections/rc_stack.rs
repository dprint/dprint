use std::rc::Rc;

use crate::formatting::PrintItemPath;

#[derive(Clone)]
struct RcNode {
  path: PrintItemPath,
  next: Option<Rc<RcNode>>,
}

#[derive(Default, Clone)]
pub struct RcStack(Option<Rc<RcNode>>);

impl RcStack {
  pub fn push(&mut self, path: PrintItemPath) {
    let next = self.0.as_ref().map(Rc::clone);
    self.0 = Some(Rc::new(RcNode { path, next }));
  }

  pub fn pop(&mut self) -> Option<PrintItemPath> {
    let head = Rc::clone(self.0.as_ref()?);
    self.0 = head.next.as_ref().cloned();
    Some(head.path)
  }

  pub fn is_empty(&self) -> bool {
    self.0.is_none()
  }
}
