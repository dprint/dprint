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
