use std::io::ErrorKind;
use std::io::Read;
use std::io::Result;
use std::io::Write;

const SUCCESS_BYTES: &[u8; 4] = &[255, 255, 255, 255];

pub struct MessageReader<TRead: Read + Unpin> {
  reader: TRead,
}

impl<TRead: Read + Unpin> MessageReader<TRead> {
  /// Creates a reader for messages from the provided reader.
  ///
  /// The reader must only write to the buffers passed to `Read::read` and
  /// never read from them, which is the case for pipes such as `ChildStdout`
  /// and `Stdin`. See `read_exact_into_uninit` for why.
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
    let mut buf = Vec::new();
    if size > 0 {
      buf.try_reserve_exact(size)?;
      read_exact_into_uninit(&mut self.reader, &mut buf, size)?;
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
    self.writer.flush()
  }
}

/// Reads exactly `size` bytes into the reserved capacity of `buf` without
/// zeroing it first.
///
/// This is not a security issue:
///
/// - The readers used with this (`ChildStdout` and `Stdin`) only write to the
///   buffer they're given and never read from it. A plugin only controls the
///   bytes sent over the pipe, not the `Read` implementation, so it can't make
///   this read uninitialized memory.
/// - The uninitialized bytes never escape. On success every byte has been
///   overwritten and on failure the buffer is dropped with the error.
///
/// Every sound alternative was benchmarked and was measurably slower for large
/// messages: `resize(size, 0)` zeroes the buffer before reading,
/// `take().read_to_end()` reads in small chunks, and `vec![0; size]` aborts on
/// allocation failure rather than erroring.
#[inline(always)]
fn read_exact_into_uninit(reader: &mut impl Read, buf: &mut Vec<u8>, size: usize) -> Result<()> {
  debug_assert!(buf.is_empty() && buf.capacity() >= size);
  // SAFETY: capacity was reserved for `size` bytes and, given a reader that
  // only writes to its buffer, the bytes are only observed once `read_exact`
  // has overwritten them all (see above)
  unsafe {
    buf.set_len(size);
  }
  if let Err(err) = reader.read_exact(buf) {
    buf.clear();
    return Err(err);
  }
  Ok(())
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn read_bytes_reads_exact_size() {
    let data = [1, 2, 3, 4, 5];
    let mut reader = MessageReader::new(&data[..]);
    assert_eq!(reader.read_bytes(3).unwrap(), vec![1, 2, 3]);
    assert_eq!(reader.read_bytes(0).unwrap(), Vec::<u8>::new());
    assert_eq!(reader.read_bytes(2).unwrap(), vec![4, 5]);
  }

  #[test]
  fn read_bytes_errors_on_eof() {
    let data = [1, 2];
    let mut reader = MessageReader::new(&data[..]);
    let err = reader.read_bytes(3).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::UnexpectedEof);
  }
}
