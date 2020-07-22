use std::io::{Read, Write};
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

    /// Sends the message kind (4 bytes).
    pub fn send_message_kind(&mut self, message_kind: u32) -> Result<(), ErrBox> {
        self.writer.write_all(&message_kind.to_be_bytes())?;
        self.writer.flush()?;

        Ok(())
    }

    /// Sends the message part (4 bytes length, X bytes data)
    /// Messages may have multiple parts.
    pub fn send_message_part(&mut self, data: &[u8]) -> Result<(), ErrBox> {
        // send the message part length (4 bytes)
        self.writer.write_all(&(data.len() as u32).to_be_bytes())?;

        // write first part of data to writer buffer
        self.writer.write_all(&data[0..std::cmp::min(BUFFER_SIZE, data.len())])?;
        self.writer.flush()?;

        // write remaining bytes
        let mut index = BUFFER_SIZE;
        while index < data.len() {
            // wait for "ready byte" from the client
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

    /// Gets the message kind (4 bytes)
    pub fn read_message_kind(&mut self) -> Result<u32, ErrBox> {
        self.read_u32()
    }

    pub fn read_message_part_as_u32(&mut self) -> Result<u32, ErrBox> {
        let message_part = self.read_message_part()?;
        if message_part.len() != 4 {
            return err!("Expected to read a message part size of 4 bytes, but was {}.", message_part.len());
        }
        let mut dst = [0u8; 4];
        dst.clone_from_slice(&message_part[0..4]);
        Ok(u32::from_be_bytes(dst))
    }

    pub fn read_message_part_as_string(&mut self) -> Result<String, ErrBox> {
        let message_part = self.read_message_part()?;
        Ok(String::from_utf8(message_part)?)
    }

    /// Gets the message part (4 bytes length, X bytes data)
    /// Messages may have multiple parts.
    pub fn read_message_part(&mut self) -> Result<Vec<u8>, ErrBox> {
        let size = self.read_u32()? as usize;

        let mut message_data = vec![0u8; size];
        if size > 0 {
            // read first part of response
            self.reader.read_exact(&mut message_data[0..std::cmp::min(BUFFER_SIZE, size)])?;

            // read remaining bytes
            let mut index = BUFFER_SIZE;
            while index < size {
                // send "ready byte" to the client
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

    fn read_u32(&mut self) -> Result<u32, ErrBox> {
        let mut int_buf: [u8; 4] = [0; 4];
        self.reader.read_exact(&mut int_buf)?;
        Ok(u32::from_be_bytes(int_buf))
    }
}
