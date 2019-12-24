use super::super::*;

pub fn get_use_new_lines_for_nodes(first_node: &mut TextRange, second_node: &mut TextRange) -> bool {
    return first_node.end_line() != second_node.start_line();
}

