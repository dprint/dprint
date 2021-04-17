// todo: thread_local?
#[cfg(feature = "tracing")]
static GRAPH_NODE_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[derive(Clone)]
pub struct GraphNode<'a, T> {
    item: T,
    previous: Option<&'a GraphNode<'a, T>>,
    #[cfg(feature = "tracing")]
    pub graph_node_id: usize,
}

impl<'a, T> GraphNode<'a, T> {
    pub fn new(item: T, previous: Option<&'a GraphNode<'a, T>>) -> GraphNode<'a, T> {
        GraphNode {
            item,
            previous,
            #[cfg(feature = "tracing")]
            graph_node_id: GRAPH_NODE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst),
        }
    }

    #[cfg(any(feature = "tracing", debug_assertions))]
    pub fn borrow_item(&self) -> &T {
        &self.item
    }

    pub fn borrow_previous(&self) -> &Option<&'a GraphNode<'a, T>> {
        &self.previous
    }
}

impl<'a, T> IntoIterator for &'a GraphNode<'a, T> {
    type Item = &'a T;
    type IntoIter = GraphNodeIterator<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        GraphNodeIterator {
            node: Some(self),
        }
    }
}

pub struct GraphNodeIterator<'a, T> {
    node: Option<&'a GraphNode<'a, T>>,
}

impl<'a, T> GraphNodeIterator<'a, T> {
    pub fn empty() -> GraphNodeIterator<'a, T> {
        GraphNodeIterator {
            node: None,
        }
    }
}

impl<'a, T> Iterator for GraphNodeIterator<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<&'a T> {
        match self.node.take() {
            Some(node) => {
                self.node = node.previous;
                Some(&node.item)
            },
            None => None
        }
    }
}
