use super::super::*;

pub fn has_separating_blank_line(first_node: &dyn Ranged, second_node: &dyn Ranged, context: &mut Context) -> bool {
    return get_second_start_line(first_node, second_node, context) > first_node.end_line(context) + 1;

    fn get_second_start_line(first_node: &dyn Ranged, second_node: &dyn Ranged, context: &mut Context) -> usize {
        let leading_comments = second_node.leading_comments(context);

        for comment in leading_comments {
            let comment_start_line = comment.start_line(context);
            if comment_start_line > first_node.end_line(context) {
                return comment_start_line;
            }
        }

        second_node.start_line(context)
    }
}