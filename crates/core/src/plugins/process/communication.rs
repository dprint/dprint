use anyhow::bail;
use anyhow::Result;
use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWrite;
use tokio::io::AsyncWriteExt;

const SUCCESS_BYTES: &[u8; 4] = &[255, 255, 255, 255];

pub struct MessageReader<TRead: AsyncRead + Unpin> {
  reader: TRead,
}

impl<TRead: AsyncRead + Unpin> MessageReader<TRead> {
  pub fn new(reader: TRead) -> Self {
    Self { reader }
  }

  /// Reads a u32 value.
  pub async fn read_u32(&mut self) -> Result<u32> {
    let mut int_buf: [u8; 4] = [0; 4];
    self.reader.read_exact(&mut int_buf).await?;
    Ok(u32::from_be_bytes(int_buf))
  }

  /// Reads a u32 value followed by a buffer.
  pub async fn read_sized_bytes(&mut self) -> Result<Vec<u8>> {
    let size = self.read_u32().await? as usize;
    self.read_bytes(size).await
  }

  pub async fn read_bytes(&mut self, size: usize) -> Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(size);
    if size > 0 {
      unsafe {
        buf.set_len(size);
      }
      self.reader.read_exact(&mut buf).await?;
    }
    Ok(buf)
  }

  pub async fn read_success_bytes(&mut self) -> Result<()> {
    let read_bytes = self.inner_read_success_bytes().await?;
    if &read_bytes != SUCCESS_BYTES {
      bail!(
        "Catastrophic error reading from process. Did not receive the success bytes at end of message. Found: {:?}",
        read_bytes
      )
    }
    Ok(())
  }

  async fn inner_read_success_bytes(&mut self) -> Result<[u8; 4]> {
    let mut read_buf: [u8; 4] = [0; 4];
    self.reader.read_exact(&mut read_buf).await?;
    Ok(read_buf)
  }
}

pub struct MessageWriter<TWrite: AsyncWrite + Unpin> {
  writer: TWrite,
}

impl<TWrite: AsyncWrite + Unpin> MessageWriter<TWrite> {
  pub fn new(writer: TWrite) -> Self {
    Self { writer }
  }

  pub async fn send_u32(&mut self, value: u32) -> Result<()> {
    self.writer.write_all(&value.to_be_bytes()).await?;
    Ok(())
  }

  pub async fn send_sized_bytes(&mut self, bytes: &[u8]) -> Result<()> {
    self.send_u32(bytes.len() as u32).await?;
    if !bytes.is_empty() {
      self.writer.write_all(bytes).await?;
    }
    Ok(())
  }

  pub async fn send_success_bytes(&mut self) -> Result<()> {
    self.writer.write_all(SUCCESS_BYTES).await?;
    self.writer.flush().await?;
    Ok(())
  }

  pub async fn flush(&mut self) -> Result<()> {
    Ok(self.writer.flush().await?)
  }
}
