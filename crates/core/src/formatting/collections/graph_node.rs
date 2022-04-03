#[derive(Clone)]
pub struct GraphNode<'a, T> {
  pub item: T,
  pub previous: Option<&'a GraphNode<'a, T>>,
  #[cfg(feature = "tracing")]
  pub graph_node_id: u32,
}

impl<'a, T> GraphNode<'a, T> {
  pub fn new(item: T, previous: Option<&'a GraphNode<'a, T>>) -> GraphNode<'a, T> {
    GraphNode {
      item,
      previous,
      #[cfg(feature = "tracing")]
      graph_node_id: super::super::thread_state::next_graph_node_id(),
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
