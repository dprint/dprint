use std::io::ErrorKind;
use std::io::Read;
use std::io::Result;
use std::io::Write;

const SUCCESS_BYTES: &[u8; 4] = &[255, 255, 255, 255];

pub struct MessageReader<TRead: Read + Unpin> {
  reader: TRead,
}

impl<TRead: Read + Unpin> MessageReader<TRead> {
  pub fn new(reader: TRead) -> Self {
    Self { reader }
  }

  /// Reads a u32 value.
  pub fn read_u32(&mut self) -> Result<u32> {
    let mut int_buf: [u8; 4] = [0; 4];
    self.reader.read_exact(&mut int_buf)?;
    Ok(u32::from_be_bytes(int_buf))
  }

  /// Reads a u32 value followed by a buffer.
  pub fn read_sized_bytes(&mut self) -> Result<Vec<u8>> {
    let size = self.read_u32()? as usize;
    self.read_bytes(size)
  }

  #[allow(clippy::read_zero_byte_vec)]
  pub fn read_bytes(&mut self, size: usize) -> Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(size);
    if size > 0 {
      unsafe {
        buf.set_len(size);
      }
      self.reader.read_exact(&mut buf)?;
    }
    Ok(buf)
  }

  pub fn read_success_bytes(&mut self) -> Result<()> {
    let read_bytes = self.inner_read_success_bytes()?;
    if &read_bytes != SUCCESS_BYTES {
      let message = format!(
        "Catastrophic error reading from process. Did not receive the success bytes at end of message. Found: {:?}",
        read_bytes
      );
      Result::Err(std::io::Error::new(ErrorKind::InvalidData, message))
    } else {
      Ok(())
    }
  }

  fn inner_read_success_bytes(&mut self) -> Result<[u8; 4]> {
    let mut read_buf: [u8; 4] = [0; 4];
    self.reader.read_exact(&mut read_buf)?;
    Ok(read_buf)
  }
}

pub struct MessageWriter<TWrite: Write + Unpin> {
  writer: TWrite,
}

impl<TWrite: Write + Unpin> MessageWriter<TWrite> {
  pub fn new(writer: TWrite) -> Self {
    Self { writer }
  }

  pub fn send_u32(&mut self, value: u32) -> Result<()> {
    self.writer.write_all(&value.to_be_bytes())?;
    Ok(())
  }

  pub fn send_sized_bytes(&mut self, bytes: &[u8]) -> Result<()> {
    self.send_u32(bytes.len() as u32)?;
    if !bytes.is_empty() {
      self.writer.write_all(bytes)?;
    }
    Ok(())
  }

  pub fn send_success_bytes(&mut self) -> Result<()> {
    self.writer.write_all(SUCCESS_BYTES)?;
    self.writer.flush()?;
    Ok(())
  }

  pub fn flush(&mut self) -> Result<()> {
    Ok(self.writer.flush()?)
  }
}
