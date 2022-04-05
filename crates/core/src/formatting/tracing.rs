use super::*;
use std::collections::HashSet;

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
      PrintItem::Info(info) => TracePrintItem::Info(match info {
        Info::LineNumber(info) => TraceInfo::LineNumber(TraceInfoInner::new(info.unique_id(), info.name())),
        Info::ColumnNumber(info) => TraceInfo::ColumnNumber(TraceInfoInner::new(info.unique_id(), info.name())),
        Info::IsStartOfLine(info) => TraceInfo::IsStartOfLine(TraceInfoInner {
          info_id: info.unique_id(),
          name: info.name().to_string(),
        }),
        Info::IndentLevel(info) => TraceInfo::IndentLevel(TraceInfoInner::new(info.unique_id(), info.name())),
        Info::LineStartColumnNumber(info) => TraceInfo::LineStartColumnNumber(TraceInfoInner::new(info.unique_id(), info.name())),
        Info::LineStartIndentLevel(info) => TraceInfo::LineStartIndentLevel(TraceInfoInner::new(info.unique_id(), info.name())),
      }),
      PrintItem::Condition(condition) => {
        if let Some(true_path) = condition.get_true_path() {
          path_stack.push(true_path);
        }
        if let Some(false_path) = condition.get_false_path() {
          path_stack.push(false_path);
        }
        TracePrintItem::Condition(TraceCondition {
          condition_id: condition.unique_id(),
          name: condition.name().to_string(),
          is_stored: condition.is_stored,
          store_save_point: condition.store_save_point,
          true_path: condition.get_true_path().map(|p| p.get_node_id()),
          false_path: condition.get_false_path().map(|p| p.get_node_id()),
        })
      }
      PrintItem::Signal(signal) => TracePrintItem::Signal(signal),
      PrintItem::RcPath(path) => {
        path_stack.push(path);
        TracePrintItem::RcPath(path.get_node_id())
      }
      PrintItem::Anchor(Anchor::LineNumber(anchor)) => TracePrintItem::Anchor(TraceLineNumberAnchor {
        anchor_id: anchor.unique_id(),
        name: anchor.name().to_string(),
      }),
      PrintItem::ConditionReevaluation(reevaluation) => TracePrintItem::ConditionReevaluation(TraceConditionReevaluation {
        condition_id: reevaluation.condition_id,
        name: reevaluation.name().to_string(),
      }),
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
