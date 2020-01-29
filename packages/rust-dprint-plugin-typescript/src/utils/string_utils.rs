pub fn has_new_line_occurrences_in_leading_whitespace(text: &str, occurrences: i8) -> bool {
    if occurrences == 0 {
        return has_no_new_lines_in_leading_whitespace(text);
    }

    let mut found_occurrences = 0;
    for c in text.chars() {
        if !c.is_whitespace() {
            return false;
        }
        if c == '\n' {
            found_occurrences += 1;
            if found_occurrences >= occurrences {
                return true;
            }
        }
    }

    return false;
}

pub fn has_no_new_lines_in_leading_whitespace(text: &str) -> bool {
    for c in text.chars() {
        if !c.is_whitespace() {
            return true;
        }
        if c == '\n' {
            return false;
        }
    }

    return true;
}

pub fn has_new_line_occurrences_in_trailing_whitespace(text: &str, occurrences: i8) -> bool {
    if occurrences == 0 {
        return has_no_new_lines_in_trailing_whitespace(text);
    }

    let mut found_occurrences = 0;
    for c in text.chars().rev() {
        if !c.is_whitespace() {
            return false;
        }
        if c == '\n' {
            found_occurrences += 1;
            if found_occurrences >= occurrences {
                return true;
            }
        }
    }

    return false;
}

pub fn has_no_new_lines_in_trailing_whitespace(text: &str) -> bool {
    for c in text.chars().rev() {
        if !c.is_whitespace() {
            return true;
        }
        if c == '\n' {
            return false;
        }
    }

    return true;
}

// todo: unit tests

pub fn get_line_number_of_pos(text: &str, pos: usize) -> usize {
    let text_bytes = text.as_bytes();
    let mut line_count = 1; // 1-indexed

    for i in 0..pos {
        if text_bytes.get(i) == Some(&('\n' as u8)) {
            line_count += 1;
        }
    }

    line_count
}

pub fn get_column_number_of_pos(text: &str, pos: usize) -> usize {
    let text_bytes = text.as_bytes();
    let line_start_byte_pos = get_line_start_byte_pos(pos, &text_bytes);

    return text[line_start_byte_pos..pos].chars().count() + 1; // 1-indexed

    fn get_line_start_byte_pos(pos: usize, text_bytes: &[u8]) -> usize {
        for i in (0..pos).rev() {
            if text_bytes.get(i) == Some(&('\n' as u8)) {
                return i + 1;
            }
        }

        0
    }
}
