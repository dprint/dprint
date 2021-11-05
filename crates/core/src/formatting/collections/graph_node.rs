#[cfg(feature = "tracing")]
mod counter {
  use crate::formatting::utils::CounterCell;

  thread_local! {
    static GRAPH_NODE_COUNTER: CounterCell = CounterCell::new();
  }

  pub fn increment_and_get() -> usize {
    GRAPH_NODE_COUNTER.with(CounterCell::increment_and_get)
  }
}

#[derive(Clone)]
pub struct GraphNode<'a, T> {
  pub item: T,
  pub previous: Option<&'a GraphNode<'a, T>>,
  #[cfg(feature = "tracing")]
  pub graph_node_id: usize,
}

impl<'a, T> GraphNode<'a, T> {
  pub fn new(item: T, previous: Option<&'a GraphNode<'a, T>>) -> GraphNode<'a, T> {
    GraphNode {
      item,
      previous,
      #[cfg(feature = "tracing")]
      graph_node_id: counter::increment_and_get(),
    }
  }
}

impl<'a, T: Copy> GraphNode<'a, T> {
  pub fn iter(&'a self) -> impl DoubleEndedIterator<Item = T> {
    let mut nodes = vec![self.item];
    let mut curr = self;
    while let Some(prev) = curr.previous {
      curr = prev;
      nodes.push(curr.item);
    }
    nodes.into_iter()
  }
}
