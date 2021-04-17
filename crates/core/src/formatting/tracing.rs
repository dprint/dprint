use std::collections::HashSet;
use super::*;

/// Gets all the TracePrintNodes for analysis from the starting node.
pub fn get_trace_print_nodes(start_node: Option<PrintItemPath>) -> Vec<TracePrintNode> {
    let mut print_nodes = Vec::new();
    let mut path_stack = Vec::new();
    let mut handled_nodes = HashSet::new();

    if let Some(start_node) = start_node {
        path_stack.push(start_node);
    }

    // do not use recursion as it will easily overflow the stack
    while let Some(node) = path_stack.pop() {
        let node_id = node.get_node_id();
        if handled_nodes.contains(&node_id) {
            continue;
        }

        // get the trace print item
        let trace_print_item = match node.get_item() {
            PrintItem::String(text) => TracePrintItem::String(text.text.to_string()),
            PrintItem::Info(info) => TracePrintItem::Info(TraceInfo {
                info_id: info.get_unique_id(),
                name: info.get_name().to_string(),
            }),
            PrintItem::Condition(condition) => {
                if let Some(true_path) = condition.get_true_path() {
                    path_stack.push(true_path);
                }
                if let Some(false_path) = condition.get_false_path() {
                    path_stack.push(false_path);
                }
                TracePrintItem::Condition(TraceCondition {
                    condition_id: condition.get_unique_id(),
                    name: condition.get_name().to_string(),
                    is_stored: condition.is_stored,
                    dependent_infos: condition.dependent_infos.as_ref().map(|infos| infos.iter().map(|i| i.get_unique_id()).collect()),
                    true_path: condition.get_true_path().map(|p| p.get_node_id()),
                    false_path: condition.get_false_path().map(|p| p.get_node_id()),
                })
            },
            PrintItem::Signal(signal) => TracePrintItem::Signal(signal),
            PrintItem::RcPath(path) => {
                path_stack.push(path);
                TracePrintItem::RcPath(path.get_node_id())
            },
        };

        // create and store the trace print node
        print_nodes.push(TracePrintNode {
            print_node_id: node_id,
            next_print_node_id: node.get_next().map(|n| n.get_node_id()),
            print_item: trace_print_item,
        });

        if let Some(next) = node.get_next() {
            path_stack.push(next);
        }

        handled_nodes.insert(node_id);
    }

    print_nodes
}