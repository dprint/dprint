use super::super::configuration::Configuration;

pub struct Context<'a> {
    pub file_text: &'a str,
    pub configuration: &'a Configuration,
}

impl<'a> Context<'a> {
    pub fn new(file_text: &'a str, configuration: &'a Configuration) -> Context<'a> {
        Context {
            file_text,
            configuration,
        }
    }

    pub fn get_new_lines_in_range(&self, start: usize, end: usize) -> u32 {
        let file_bytes = self.file_text.as_bytes();
        let mut count = 0;
        for byte in &file_bytes[start..end] {
            if byte == &('\n' as u8) {
                count += 1;
            }
        }
        count
    }

    pub fn get_indent_level_at_pos(&self, pos: usize) -> u32 {
        let file_bytes = self.file_text.as_bytes();
        let mut count = 0;

        for byte in file_bytes[0..pos].iter().rev() {
            // This is ok because we are just investigating whitespace chars
            // which I believe are only 1 byte.
            let character = *byte as char;

            if character == '\n' {
                break;
            }

            if character == '\t' {
                count += self.configuration.indent_width;
            } else if character.is_whitespace() {
                count += 1;
            } else {
                // todo: unexpected... I guess break?
                break;
            }
        }

        (count as f64 / self.configuration.indent_width as f64).round() as u32
    }
}
