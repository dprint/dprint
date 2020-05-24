use std::rc::Rc;
use super::WasmFunctions;

pub struct BytesTransmitter {
    wasm_functions: Rc<WasmFunctions>,
    buffer_size: usize,
}

impl<'a> BytesTransmitter {
    pub fn new(wasm_functions: Rc<WasmFunctions>) -> Self {
        let buffer_size = wasm_functions.get_wasm_memory_buffer_size();
        BytesTransmitter {
            wasm_functions,
            buffer_size,
        }
    }

    pub fn send_string(&self, text: &str) {
        let mut index = 0;
        let len = text.len();
        let text_bytes = text.as_bytes();
        self.wasm_functions.clear_shared_bytes(len);
        while index < len {
            let write_count = std::cmp::min(len - index, self.buffer_size);
            self.write_bytes_to_memory_buffer(&text_bytes[index..(index + write_count)]);
            self.wasm_functions.add_to_shared_bytes_from_buffer(write_count);
            index += write_count;
        }
    }

    fn write_bytes_to_memory_buffer(&self, bytes: &[u8]) {
        let length = bytes.len();
        let wasm_buffer_pointer = self.wasm_functions.get_wasm_memory_buffer_ptr();
        let memory_writer = wasm_buffer_pointer
            .deref(self.wasm_functions.get_memory(), 0, length as u32)
            .unwrap();
        for i in 0..length {
            memory_writer[i].set(bytes[i]);
        }
    }

    pub fn receive_string(&self, len: usize) -> String {
        let mut index = 0;
        let mut bytes: Vec<u8> = vec![0; len];
        while index < len {
            let read_count = std::cmp::min(len - index, self.buffer_size);
            self.wasm_functions.set_buffer_with_shared_bytes(index, read_count);
            self.read_bytes_from_memory_buffer(&mut bytes[index..(index + read_count)]);
            index += read_count;
        }
        String::from_utf8(bytes).unwrap()
    }

    fn read_bytes_from_memory_buffer(&self, bytes: &mut [u8]) {
        let length = bytes.len();
        let wasm_buffer_pointer = self.wasm_functions.get_wasm_memory_buffer_ptr();
        let memory_reader = wasm_buffer_pointer
            .deref(self.wasm_functions.get_memory(), 0, length as u32)
            .unwrap();
        for i in 0..length {
            bytes[i] = memory_reader[i].get();
        }
    }
}
