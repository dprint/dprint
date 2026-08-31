// reads the built dprint binary's PE headers and reports which DLLs are
// eagerly loaded vs. delay-loaded. this exists to ensure DLLs don't
// negatively impact Windows startup performance—prefer delay loading DLLs
// that aren't needed on the common startup path.
#![cfg(windows)]

use std::collections::BTreeMap;

// keep this in sync with the delay loaded DLLs in build.rs. this exists so
// that adding a DLL to the startup path (or accidentally eager loading one
// of these) shows up as a diff. to update, run the test and copy the actual
// output printed on failure.
//
// note: `kernel32` and `ntdll` must stay eager, but the rest of the eager
// DLLs below are candidates for delay loading if they're not needed at
// startup—see build.rs.
const EXPECTED: &str = "\
advapi32.dll: eager
api-ms-win-core-synch-l1-2-0.dll: eager
bcrypt.dll: eager
bcryptprimitives.dll: eager
combase.dll: delay
crypt32.dll: delay
kernel32.dll: eager
ntdll.dll: eager
oleaut32.dll: delay
pdh.dll: delay
powrprof.dll: delay
psapi.dll: delay
shell32.dll: eager
user32.dll: eager
ws2_32.dll: delay
";

#[test]
#[allow(clippy::disallowed_methods)] // standalone test binary; no Environment available
fn windows_dll_imports() {
  let exe_path = env!("CARGO_BIN_EXE_dprint");
  let buf = std::fs::read(exe_path).unwrap();
  let pe = Pe::parse(&buf);

  // a DLL can appear under multiple import descriptors—dedupe, preferring
  // "eager" if a DLL somehow shows up in both tables
  let mut kinds: BTreeMap<String, &'static str> = BTreeMap::new();
  for name in pe.delay_dll_names() {
    kinds.insert(name, "delay");
  }
  for name in pe.eager_dll_names() {
    kinds.insert(name, "eager");
  }

  let mut actual = String::new();
  for (name, kind) in &kinds {
    actual.push_str(&format!("{name}: {kind}\n"));
  }

  pretty_assertions::assert_eq!(EXPECTED, actual);
}

struct Pe<'a> {
  buf: &'a [u8],
  sections: Vec<Section>,
  data_dirs_start: usize,
}

struct Section {
  virtual_address: u32,
  raw_size: u32,
  raw_offset: u32,
}

impl<'a> Pe<'a> {
  fn parse(buf: &'a [u8]) -> Pe<'a> {
    let pe_offset = read_u32(buf, 0x3c) as usize;
    let num_sections = read_u16(buf, pe_offset + 6) as usize;
    let size_opt_header = read_u16(buf, pe_offset + 20) as usize;
    let opt_start = pe_offset + 24;
    let magic = read_u16(buf, opt_start);
    assert_eq!(magic, 0x20b, "expected a PE32+ binary");

    // data directories start at opt_start + 112 for PE32+
    let data_dirs_start = opt_start + 112;

    let sec_start = opt_start + size_opt_header;
    let mut sections = Vec::with_capacity(num_sections);
    for i in 0..num_sections {
      let off = sec_start + i * 40;
      sections.push(Section {
        virtual_address: read_u32(buf, off + 12),
        raw_size: read_u32(buf, off + 16),
        raw_offset: read_u32(buf, off + 20),
      });
    }

    Pe {
      buf,
      sections,
      data_dirs_start,
    }
  }

  // import directory (data directory index 1): entries are 20 bytes, name RVA at offset 12
  fn eager_dll_names(&self) -> Vec<String> {
    let rva = self.data_dir_rva(1);
    self.parse_dll_names(rva, 20, 12)
  }

  // delay-import directory (data directory index 13): entries are 32 bytes, name RVA at offset 4
  fn delay_dll_names(&self) -> Vec<String> {
    let rva = self.data_dir_rva(13);
    self.parse_dll_names(rva, 32, 4)
  }

  fn data_dir_rva(&self, index: usize) -> u32 {
    // each data directory entry is 8 bytes (RVA + size)
    read_u32(self.buf, self.data_dirs_start + index * 8)
  }

  fn parse_dll_names(&self, table_rva: u32, entry_size: usize, name_rva_offset: usize) -> Vec<String> {
    let mut names = Vec::new();
    let Some(table_offset) = self.rva_to_offset(table_rva) else {
      return names;
    };
    let mut idx = 0;
    loop {
      let entry_off = table_offset + idx * entry_size;
      let name_rva = read_u32(self.buf, entry_off + name_rva_offset);
      if name_rva == 0 {
        break;
      }
      if let Some(name_off) = self.rva_to_offset(name_rva) {
        names.push(read_c_string(self.buf, name_off).to_lowercase());
      }
      idx += 1;
    }
    names
  }

  fn rva_to_offset(&self, rva: u32) -> Option<usize> {
    for s in &self.sections {
      if rva >= s.virtual_address && rva < s.virtual_address + s.raw_size {
        return Some((s.raw_offset + (rva - s.virtual_address)) as usize);
      }
    }
    None
  }
}

fn read_u16(buf: &[u8], offset: usize) -> u16 {
  u16::from_le_bytes([buf[offset], buf[offset + 1]])
}

fn read_u32(buf: &[u8], offset: usize) -> u32 {
  u32::from_le_bytes([buf[offset], buf[offset + 1], buf[offset + 2], buf[offset + 3]])
}

fn read_c_string(buf: &[u8], offset: usize) -> String {
  let mut end = offset;
  while end < buf.len() && buf[end] != 0 {
    end += 1;
  }
  String::from_utf8_lossy(&buf[offset..end]).into_owned()
}
