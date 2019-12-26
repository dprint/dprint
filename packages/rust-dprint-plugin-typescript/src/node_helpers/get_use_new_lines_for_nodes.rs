use super::super::*;

pub fn get_use_new_lines_for_nodes(first_node: &dyn Ranged, second_node: &dyn Ranged, context: &mut Context) -> bool {
    return first_node.end_line(context) != second_node.start_line(context);
}

