use super::*;
use swc_common::{comments::{Comment}, BytePos};

pub fn is_first_node_on_line(node: &dyn Ranged, context: &mut Context) -> bool {
    let start = node.lo().0 as usize;

    for i in (0..start).rev() {
        let c = context.file_bytes[i];
        if c != ' ' as u8 && c != '\t' as u8 {
            return c == '\n' as u8;
        }
    }

    return true;
}

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

pub fn get_use_new_lines_for_nodes(first_node: &dyn Ranged, second_node: &dyn Ranged, context: &mut Context) -> bool {
    return first_node.end_line(context) != second_node.start_line(context);
}

pub fn has_leading_comment_on_different_line<'a>(node: &dyn Ranged, comments_to_ignore: Option<&Vec<&'a Comment>>, context: &mut Context<'a>) -> bool {
    get_leading_comment_on_different_line(node, comments_to_ignore, context).is_some()
}

pub fn get_leading_comment_on_different_line<'a>(node: &dyn Ranged, comments_to_ignore: Option<&Vec<&'a Comment>>, context: &mut Context<'a>) -> Option<&'a Comment> {
    let comments_to_ignore: Option<Vec<BytePos>> = comments_to_ignore.map(|x| x.iter().map(|c| c.lo()).collect());
    let node_start_line = node.start_line(context);
    for comment in node.leading_comments(context) {
        if let Some(comments_to_ignore) = &comments_to_ignore {
            if comments_to_ignore.contains(&comment.lo()) {
                continue;
            }
        }

        if comment.start_line(context) < node_start_line {
            return Some(comment);
        }
    }

    return None;
}
