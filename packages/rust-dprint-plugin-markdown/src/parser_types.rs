pub struct Context<'a> {
    pub file_text: &'a str,
}

impl<'a> Context<'a> {
    pub fn new(file_text: &'a str) -> Context<'a> {
        Context {
            file_text,
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
}
