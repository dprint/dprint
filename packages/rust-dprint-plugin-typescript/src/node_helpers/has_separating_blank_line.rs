use super::super::*;

pub fn has_separating_blank_line(first_node: &mut TextRange, second_node: &mut TextRange, context: &mut Context) -> bool {
    return get_second_start_line(first_node, second_node, context) > first_node.end_line() + 1;

    fn get_second_start_line(first_node: &mut TextRange, second_node: &mut TextRange, context: &mut Context) -> usize {
        let leading_comments = second_node.leading_comments();

        for comment in leading_comments {
            let mut comment_range = context.get_text_range(&comment.span);
            if comment_range.start_line() > first_node.end_line() {
                return comment_range.start_line();
            }
        }

        second_node.end_line()
    }
}