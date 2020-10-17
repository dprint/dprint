pub const BOM_CHAR: char = '\u{FEFF}';

pub struct FileText {
    text: String,
}

impl FileText {
    pub fn new(text: String) -> Self {
        FileText { text }
    }

    pub fn has_bom(&self) -> bool {
        self.text.starts_with(BOM_CHAR)
    }

    pub fn as_str(&self) -> &str {
        if self.has_bom() {
            // strip BOM
            &self.text[BOM_CHAR.len_utf8()..]
        } else {
            &self.text
        }
    }
}
