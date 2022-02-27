use anyhow::bail;
use anyhow::Result;
use std::io::Read;
use std::io::Write;

const SUCCESS_BYTES: &[u8; 4] = &[255, 255, 255, 255];

pub struct MessageReader<TRead: Read> {
  reader: TRead,
}

impl<TRead: Read> MessageReader<TRead> {
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

  pub fn read_bytes(&mut self, size: usize) -> Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(size);
    unsafe {
      buf.set_len(size);
    }
    self.reader.read_exact(&mut buf)?;
    Ok(buf)
  }

  pub fn read_success_bytes(&mut self) -> Result<()> {
    let read_bytes = self.inner_read_success_bytes()?;
    if &read_bytes != SUCCESS_BYTES {
      bail!(
        "Catastrophic error reading from process. Did not receive the success bytes at end of message. Found: {:?}",
        read_bytes
      )
    }
    Ok(())
  }

  pub fn read_success_bytes_with_message_on_error(&mut self, maybe_read_error_message: &[u8]) -> Result<()> {
    let read_bytes = self.inner_read_success_bytes()?;
    if &read_bytes != SUCCESS_BYTES {
      let message = "Catastrophic error reading from process. Did not receive the success bytes at end of message.";
      // attempt to convert the error message to a string
      match std::str::from_utf8(maybe_read_error_message) {
        Ok(error_message) => bail!("{} Found: {:?}. Received partial error: {}", message, read_bytes, error_message),
        Err(_) => bail!("{}", message),
      }
    }
    Ok(())
  }

  fn inner_read_success_bytes(&mut self) -> Result<[u8; 4]> {
    let mut read_buf: [u8; 4] = [0; 4];
    self.reader.read_exact(&mut read_buf)?;
    Ok(read_buf)
  }
}

pub struct MessageWriter<TWrite: Write> {
  writer: TWrite,
}

impl<TWrite: Write> MessageWriter<TWrite> {
  pub fn new(writer: TWrite) -> Self {
    Self { writer }
  }

  pub fn send_u32(&mut self, value: u32) -> Result<()> {
    self.writer.write_all(&value.to_be_bytes())?;
    Ok(())
  }

  pub fn send_sized_bytes(&mut self, bytes: &[u8]) -> Result<()> {
    self.send_u32(bytes.len() as u32)?;
    self.writer.write_all(bytes)?;
    Ok(())
  }

  pub fn send_success_bytes(&mut self) -> Result<()> {
    self.writer.write_all(SUCCESS_BYTES)?;
    self.writer.flush()?;
    Ok(())
  }
}
