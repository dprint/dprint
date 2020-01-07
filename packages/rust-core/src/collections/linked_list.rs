use std::rc::Rc;
use std::mem::{self, MaybeUninit};

pub struct Node<T> {
    previous: Option<Rc<Node<T>>>,
    item: T,
}

impl<T> Node<T> {
    pub fn new(item: T, previous: Option<Rc<Node<T>>>) -> Node<T> {
        Node {
            item,
            previous,
        }
    }

    /// Takes the item and previous item out of the node by bypassing
    /// the `Drop` implementation since properties cannot be moved out
    /// of objects that implement `Drop`.
    fn take(mut self) -> (T, Option<Rc<Node<T>>>) {
        // See here: https://phaazon.net/blog/rust-no-drop
        let item = mem::replace(&mut self.item, unsafe { MaybeUninit::zeroed().assume_init() });
        let previous = mem::replace(&mut self.previous, None);

        mem::forget(self);

        (item, previous)
    }
}

// Drop needs to be manually implemented because otherwise it
// will overflow the stack when dropping the item.
// Read more: https://stackoverflow.com/questions/28660362/thread-main-has-overflowed-its-stack-when-constructing-a-large-tree
impl<T> Drop for Node<T> {
    fn drop(&mut self) {
        let mut previous = mem::replace(&mut self.previous, None);

        loop {
            previous = match previous {
                Some(l) => {
                    match Rc::try_unwrap(l) {
                        Ok(mut l) => mem::replace(&mut l.previous, None),
                        Err(_) => break,
                    }
                },
                None => break
            }
        }
    }
}

impl<T> IntoIterator for Node<T> {
    type Item = T;
    type IntoIter = NodeIterator<T>;

    fn into_iter(self) -> Self::IntoIter {
        NodeIterator {
            node: Some(self),
        }
    }
}

pub struct NodeIterator<T> {
    node: Option<Node<T>>,
}

impl<T> NodeIterator<T> {
    pub fn empty() -> NodeIterator<T> {
        NodeIterator {
            node: None,
        }
    }
}

impl<T> Iterator for NodeIterator<T> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        let node = mem::replace(&mut self.node, None);
        match node {
            Some(node) => {
                let (item, previous) = node.take();

                self.node = previous.map(
                    |x| Rc::try_unwrap(x).ok()
                        .expect("Need to drop the other reference before iterating over the final iterator.")
                );

                Some(item)
            },
            None => None
        }
    }
}
