use std::cell::UnsafeCell;

pub struct GraphNode<'a, T: Clone> {
  pub item: T,
  previous: UnsafeCell<Option<&'a GraphNode<'a, T>>>,
  #[cfg(feature = "tracing")]
  pub graph_node_id: u32,
}

impl<'a, T: Clone> Clone for GraphNode<'a, T> {
  fn clone(&self) -> Self {
    Self {
      item: self.item.clone(),
      previous: UnsafeCell::new(unsafe { (*self.previous.get()).clone() }),
      #[cfg(feature = "tracing")]
      graph_node_id: self.graph_node_id.clone(),
    }
  }
}

impl<'a, T: Clone> GraphNode<'a, T> {
  pub fn new(item: T, previous: Option<&'a GraphNode<'a, T>>) -> GraphNode<'a, T> {
    GraphNode {
      item,
      previous: UnsafeCell::new(previous),
      #[cfg(feature = "tracing")]
      graph_node_id: super::super::thread_state::next_graph_node_id(),
    }
  }

  pub fn set_previous(&self, new_ref: Option<&'a GraphNode<'a, T>>) {
    // Should be ok because the graph node is stored bump allocated
    unsafe {
      *self.previous.get() = new_ref;
    }
  }

  pub fn previous(&self) -> Option<&'a GraphNode<'a, T>> {
    unsafe { *self.previous.get() }
  }
}

impl<'a, T: Copy> GraphNode<'a, T> {
  pub fn iter(&'a self) -> impl DoubleEndedIterator<Item = T> {
    let mut nodes = vec![self.item];
    let mut curr = self;
    while let Some(prev) = curr.previous() {
      curr = prev;
      nodes.push(curr.item);
    }
    nodes.into_iter()
  }
}
