// \u{FEFF}
pub const BOM_BYTES: &[u8] = &[0xEF, 0xBB, 0xBF];

pub struct FileText {
  text: Vec<u8>,
}

impl FileText {
  pub fn new(text: Vec<u8>) -> Self {
    FileText { text }
  }

  pub fn has_bom(&self) -> bool {
    self.text.starts_with(BOM_BYTES)
  }

  pub fn as_ref(&self) -> &[u8] {
    if self.has_bom() {
      // strip BOM
      &self.text[BOM_BYTES.len()..]
    } else {
      &self.text
    }
  }
}
