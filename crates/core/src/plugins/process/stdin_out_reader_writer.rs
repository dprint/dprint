use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use crate::types::ErrBox;

const BUFFER_SIZE: usize = 1024; // safe to assume

// todo: unit tests

pub struct StdInOutReaderWriter<'a, TRead: Read, TWrite: Write> {
    writer: &'a mut TWrite,
    reader: &'a mut TRead,
}

impl<'a, TRead: Read, TWrite: Write> StdInOutReaderWriter<'a, TRead, TWrite> {
    pub fn new(reader: &'a mut TRead, writer: &'a mut TWrite) -> Self {
        StdInOutReaderWriter {
            writer,
            reader
        }
    }

    /// Send a u32 value.
    pub fn send_u32(&mut self, value: u32) -> Result<(), ErrBox> {
        self.writer.write_all(&value.to_be_bytes())?;
        self.writer.flush()?;

        Ok(())
    }

    /// Reads a u32 value.
    pub fn read_u32(&mut self) -> Result<u32, ErrBox> {
        let mut int_buf: [u8; 4] = [0; 4];
        self.reader.read_exact(&mut int_buf)?;
        Ok(u32::from_be_bytes(int_buf))
    }

    pub fn send_string(&mut self, text: &str) -> Result<(), ErrBox> {
        self.send_variable_data(text.as_bytes())
    }

    pub fn send_path_buf(&mut self, path_buf: &Path) -> Result<(), ErrBox> {
        self.send_string(&path_buf.to_string_lossy())
    }

    /// Sends variable width data (4 bytes length, X bytes data)
    pub fn send_variable_data(&mut self, data: &[u8]) -> Result<(), ErrBox> {
        // send the message part length (4 bytes)
        self.writer.write_all(&(data.len() as u32).to_be_bytes())?;

        // write first part of data to writer buffer
        self.writer.write_all(&data[0..std::cmp::min(BUFFER_SIZE, data.len())])?;
        self.writer.flush()?;

        // write remaining bytes
        let mut index = BUFFER_SIZE;
        while index < data.len() {
            // wait for "ready" from the client
            self.reader.read_exact(&mut [0; 4])?;

            // write to buffer
            let start_index = index;
            let end_index = std::cmp::min(index + BUFFER_SIZE, data.len());
            self.writer.write_all(&data[start_index..end_index])?;
            self.writer.flush()?;

            index += BUFFER_SIZE;
        }

        Ok(())
    }

    pub fn read_string(&mut self) -> Result<String, ErrBox> {
        let message_part = self.read_variable_data()?;
        Ok(String::from_utf8(message_part)?)
    }

    pub fn read_path_buf(&mut self) -> Result<PathBuf, ErrBox> {
        let message_data = self.read_variable_data()?;
        Ok(PathBuf::from(std::str::from_utf8(&message_data)?))
    }

    /// Gets the message part (4 bytes length, X bytes data)
    /// Messages may have multiple parts.
    pub fn read_variable_data(&mut self) -> Result<Vec<u8>, ErrBox> {
        let size = self.read_u32()? as usize;

        let mut message_data = vec![0u8; size];
        if size > 0 {
            // read first part of response
            self.reader.read_exact(&mut message_data[0..std::cmp::min(BUFFER_SIZE, size)])?;

            // read remaining bytes
            let mut index = BUFFER_SIZE;
            while index < size {
                // send "ready" to the client
                self.writer.write_all(&[0; 4])?;
                self.writer.flush()?;

                // read from buffer
                let start_index = index;
                let end_index = std::cmp::min(index + BUFFER_SIZE, size);
                self.reader.read_exact(&mut message_data[start_index..end_index])?;

                index += BUFFER_SIZE;
            }
        }

        Ok(message_data)
    }
}
