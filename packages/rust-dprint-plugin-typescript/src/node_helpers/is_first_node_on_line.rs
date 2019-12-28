use super::super::*;

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