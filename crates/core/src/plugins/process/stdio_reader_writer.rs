use std::io::{Read, Write};
use crate::types::ErrBox;

const BUFFER_SIZE: usize = 1024; // safe to assume

const SUCCESS_BYTES: &[u8; 4] = &[255, 255, 255, 255];
// todo: unit tests

pub struct StdIoReaderWriter<TRead: Read, TWrite: Write> {
    writer: TWrite,
    reader: TRead,
}

impl<TRead: Read, TWrite: Write> StdIoReaderWriter<TRead, TWrite> {
    pub fn new(reader: TRead, writer: TWrite) -> Self {
        StdIoReaderWriter {
            writer,
            reader
        }
    }

    /// Send a u32 value.
    pub fn send_u32(&mut self, value: u32) -> Result<(), ErrBox> {
        self.writer.write_all(&value.to_be_bytes())?;

        Ok(())
    }

    /// Reads a u32 value.
    pub fn read_u32(&mut self) -> Result<u32, ErrBox> {
        let mut int_buf: [u8; 4] = [0; 4];
        self.reader.read_exact(&mut int_buf)?;
        Ok(u32::from_be_bytes(int_buf))
    }

    pub fn send_success_bytes(&mut self) -> Result<(), ErrBox> {
        self.writer.write_all(SUCCESS_BYTES)?;
        self.writer.flush()?;

        Ok(())
    }

    pub fn read_success_bytes(&mut self) -> Result<(), ErrBox> {
        let read_bytes = self.inner_read_success_bytes()?;
        if &read_bytes == SUCCESS_BYTES {
            Ok(())
        } else {
            panic!("Catastrophic error reading from process. Did not receive the success bytes at end of message. Found: {:?}", read_bytes)
        }
    }

    pub fn read_success_bytes_with_message_on_error(&mut self, maybe_read_error_message: &Vec<u8>) -> Result<(), ErrBox> {
        let read_bytes = self.inner_read_success_bytes()?;
        if &read_bytes == SUCCESS_BYTES {
            Ok(())
        } else {
            let message = "Catastrophic error reading from process. Did not receive the success bytes at end of message.";
            // attempt to convert the error message to a string
            match std::str::from_utf8(maybe_read_error_message) {
                Ok(error_message) => panic!("{} Found: {:?}. Received partial error: {}", message, read_bytes, error_message),
                Err(_) => panic!("{}", message),
            }
        }
    }

    fn inner_read_success_bytes(&mut self) -> Result<[u8; 4], ErrBox> {
        let mut read_buf: [u8; 4] = [0; 4];
        self.reader.read_exact(&mut read_buf)?;
        Ok(read_buf)
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
