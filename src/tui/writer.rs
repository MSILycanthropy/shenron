use std::io::{self, Write};

/// A write that wraps an SSH session's channel
pub struct SessionWriter {
    buffer: Vec<u8>,
}

impl SessionWriter {
    pub fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(4096),
        }
    }

    pub fn take(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.buffer)
    }
}

impl Write for SessionWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(buf);

        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
