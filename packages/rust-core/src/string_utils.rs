pub fn get_first_line_width(text: &str) -> u32 {
    let mut last_char = ' '; // dummy char that's not \r
    let mut index = 0;
    for c in text.chars() {
        if c == '\n' {
            if last_char == '\r' {
                return index - 1;
            } else {
                return index;
            }
        }
        index += 1;
        last_char = c;
    }

    index
}

#[cfg(test)]
mod tests {
    use super::get_first_line_width;

    #[test]
    fn it_gets_for_multi_line_slash_r_slash_n() {
        assert_eq!(get_first_line_width("test\r\ns"), 4);
    }

    #[test]
    fn it_gets_for_multi_line_slash_n() {
        assert_eq!(get_first_line_width("test\ns"), 4);
    }

    #[test]
    fn it_gets_for_single_line() {
        assert_eq!(get_first_line_width("test"), 4);
    }
}
